//! Command registry and execution system for the developer console
//!
//! This module provides a two-phase command system:
//! 1. **Declaration**: Game commands are declared at startup with name, description, and optional verbose help
//! 2. **Registration**: Handlers are attached to declared commands within specific scopes (scene, game, etc.)
//!
//! Commands that are declared but have no active handler are "noop" - they exist in help
//! but print a message that they have no effect in the current context.

use std::collections::HashMap;

use super::ConsoleState;
use crate::core::capture::CaptureTarget;
use crate::core::config::DebugConfig;

/// Result of executing a console command
#[derive(Debug, Clone, PartialEq)]
pub enum CommandResult {
  /// Command executed successfully
  Ok,
  /// Command failed with an error message
  Error(String),
  /// Command requests the game to quit
  Quit,
}

/// Type alias for command handler functions
///
/// The handler receives:
/// - `args`: Slice of argument strings (does not include the command name)
/// - `ctx`: Mutable context for accessing console output and debug config
pub type CommandFn = Box<dyn Fn(&[&str], &mut CommandContext) -> CommandResult + Send + Sync>;

/// A built-in engine command (always has a handler)
pub struct BuiltinCommand {
  /// The command name (how it's invoked)
  pub name: &'static str,
  /// A brief description of what the command does
  pub description: &'static str,
  /// The handler function
  pub handler: CommandFn,
}

impl std::fmt::Debug for BuiltinCommand {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("BuiltinCommand")
      .field("name", &self.name)
      .field("description", &self.description)
      .finish()
  }
}

/// A declared game command (noop until a handler is registered)
pub struct DeclaredCommand {
  /// The command name (how it's invoked)
  pub name: String,
  /// A brief description shown in command listings
  pub description: String,
  /// Optional verbose description shown when user types `help <command>`
  pub verbose_description: Option<String>,
  /// The active handler, if any, along with its scope ID
  pub handler: Option<(u64, CommandFn)>,
}

impl std::fmt::Debug for DeclaredCommand {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("DeclaredCommand")
      .field("name", &self.name)
      .field("description", &self.description)
      .field("has_handler", &self.handler.is_some())
      .finish()
  }
}

/// Info about a command for help display
#[derive(Debug, Clone)]
pub struct CommandInfo {
  /// The command name
  pub name: String,
  /// The command description
  pub description: String,
  /// Optional verbose description
  pub verbose_description: Option<String>,
  /// Whether the command has an active handler (false = noop)
  pub is_active: bool,
}

/// Context passed to command handlers
///
/// Provides access to console output and debug configuration.
pub struct CommandContext<'a> {
  /// The console state for printing output
  pub console: &'a mut ConsoleState,
  /// Debug configuration (mutable for commands like `fps`, `colliders`)
  pub debug_config: &'a mut DebugConfig,
  /// Request to quit the application
  pub quit_requested: bool,
  /// Frame capture requested by `debug:capture` (handled by the engine)
  pub capture_requested: Option<CaptureTarget>,
  /// Current time scale (mutable for `timescale` command)
  pub time_scale: &'a mut f32,
  /// List of all commands (for help)
  pub commands: Vec<CommandInfo>,
  /// Game name (from Ryo.toml [project] section)
  pub game_name: Option<String>,
  /// Game version (from Ryo.toml [project] section)
  pub game_version: Option<String>,
}

/// Builder for configuring console commands at the engine level.
///
/// Passed to the closure in [`EngineBuilder::with_console_commands`] to allow
/// declaring game commands and removing built-in commands before the engine starts.
///
/// # Example
///
/// ```rust,ignore
/// EngineBuilder::new()
///     .with_console_commands(|cmd| {
///         cmd.declare("game:reset", "Reset the game to initial state");
///         cmd.declare("game:godmode", "Toggle invincibility");
///         cmd.declare_verbose(
///             "game:spawn",
///             "Spawn an entity",
///             "Usage: game:spawn <enemy|item> [x] [y]",
///         );
///         cmd.remove_builtin("system:ip:wan");
///     })
///     .with_scene(GameScene::new())
///     .build()
///     .run()?;
/// ```
#[derive(Debug, Default)]
pub struct ConsoleCommandBuilder {
  /// Commands to declare (name, description, verbose_description)
  pub(crate) declarations: Vec<(String, String, Option<String>)>,
  /// Built-in commands to remove
  pub(crate) removals: Vec<String>,
}

impl ConsoleCommandBuilder {
  /// Declare a game command with a name and description.
  ///
  /// The command is registered as inactive (noop) until a handler is attached
  /// via [`CommandRegistry::register_handler`] in a game or scene context.
  pub fn declare(&mut self, name: impl Into<String>, description: impl Into<String>) {
    self
      .declarations
      .push((name.into(), description.into(), None));
  }

  /// Declare a game command with a verbose help description.
  ///
  /// The verbose description is shown when the user types `help <command>`.
  pub fn declare_verbose(
    &mut self,
    name: impl Into<String>,
    description: impl Into<String>,
    verbose: impl Into<String>,
  ) {
    self
      .declarations
      .push((name.into(), description.into(), Some(verbose.into())));
  }

  /// Remove a built-in console command by name.
  ///
  /// This allows games to disable reserved engine commands
  /// (e.g., `system:ip:lan`, `debug:fps`). After removal, you can
  /// re-declare the command as a game command with custom behavior.
  pub fn remove_builtin(&mut self, name: impl Into<String>) {
    self.removals.push(name.into());
  }
}

/// Registry of console commands
///
/// Manages both built-in engine commands and declared game commands.
pub struct CommandRegistry {
  /// Built-in commands (always have handlers)
  builtins: HashMap<&'static str, BuiltinCommand>,
  /// Declared game commands (may or may not have active handlers)
  declared: HashMap<String, DeclaredCommand>,
}

impl Default for CommandRegistry {
  fn default() -> Self {
    Self::new()
  }
}

impl std::fmt::Debug for CommandRegistry {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("CommandRegistry")
      .field("builtin_count", &self.builtins.len())
      .field("declared_count", &self.declared.len())
      .finish()
  }
}

impl CommandRegistry {
  /// Create a new empty command registry
  pub fn new() -> Self {
    Self {
      builtins: HashMap::new(),
      declared: HashMap::new(),
    }
  }

  /// Register a built-in engine command (always has a handler)
  ///
  /// Used internally for engine commands like `help`, `clear`, `debug:fps`, etc.
  pub fn register<F>(&mut self, name: &'static str, description: &'static str, handler: F)
  where
    F: Fn(&[&str], &mut CommandContext) -> CommandResult + Send + Sync + 'static,
  {
    let command = BuiltinCommand {
      name,
      description,
      handler: Box::new(handler),
    };
    self.builtins.insert(name, command);
  }

  /// Declare a game command
  ///
  /// Declared commands appear in help but are noop until a handler is registered.
  /// This should be called at game initialization to define all game commands.
  ///
  /// # Arguments
  ///
  /// * `name` - The command name (should start with "game:" prefix)
  /// * `description` - Brief description shown in command listings
  /// * `verbose_description` - Optional detailed help shown for `help <command>`
  ///
  /// # Example
  ///
  /// ```ignore
  /// ctx.declare_console_command(
  ///     "game:reset",
  ///     "Reset the game to initial state",
  ///     Some("Resets all game state including score, player position, and enemies. \
  ///           Only available during gameplay."),
  /// );
  /// ```
  pub fn declare(
    &mut self,
    name: impl Into<String>,
    description: impl Into<String>,
    verbose_description: Option<impl Into<String>>,
  ) {
    let name = name.into();
    let command = DeclaredCommand {
      name: name.clone(),
      description: description.into(),
      verbose_description: verbose_description.map(|v| v.into()),
      handler: None,
    };
    self.declared.insert(name, command);
  }

  /// Register a handler for a declared game command
  ///
  /// The handler is associated with a scope ID for automatic cleanup.
  /// When the scope ends (e.g., scene exits), call `clear_scope` to remove handlers.
  ///
  /// # Arguments
  ///
  /// * `name` - The command name (must have been declared first)
  /// * `scope_id` - Unique identifier for the scope (scene, game level, etc.)
  /// * `handler` - The function to execute when the command is invoked
  ///
  /// # Returns
  ///
  /// `true` if the command was declared and handler was attached, `false` otherwise.
  pub fn register_handler<F>(&mut self, name: &str, scope_id: u64, handler: F) -> bool
  where
    F: Fn(&[&str], &mut CommandContext) -> CommandResult + Send + Sync + 'static,
  {
    if let Some(cmd) = self.declared.get_mut(name) {
      cmd.handler = Some((scope_id, Box::new(handler)));
      true
    } else {
      false
    }
  }

  /// Unregister the handler for a declared game command.
  ///
  /// Returns the command to noop state (still visible in help but has no effect).
  pub fn unregister_handler(&mut self, name: &str) {
    if let Some(cmd) = self.declared.get_mut(name) {
      cmd.handler = None;
    }
  }

  /// Remove a built-in command entirely.
  ///
  /// This allows games to override or disable reserved engine commands.
  /// Returns `true` if the command existed and was removed.
  pub fn remove_builtin(&mut self, name: &str) -> bool {
    self.builtins.remove(name).is_some()
  }

  /// Clear all handlers registered with the given scope ID
  ///
  /// Called when a scope ends (e.g., scene exits) to cleanup handlers.
  pub fn clear_scope(&mut self, scope_id: u64) {
    for cmd in self.declared.values_mut() {
      if let Some((handler_scope, _)) = &cmd.handler
        && *handler_scope == scope_id
      {
        cmd.handler = None;
      }
    }
  }

  /// Execute a command string
  ///
  /// Parses the input and invokes the appropriate command handler.
  /// For declared commands without handlers, prints a noop message.
  pub fn execute(&self, input: &str, ctx: &mut CommandContext) -> CommandResult {
    let tokens: Vec<&str> = input.split_whitespace().collect();

    if tokens.is_empty() {
      return CommandResult::Ok;
    }

    let command_name = tokens[0].to_lowercase();
    let args: Vec<&str> = tokens[1..].to_vec();

    // Check built-in commands first
    if let Some(command) = self.builtins.get(command_name.as_str()) {
      return (command.handler)(&args, ctx);
    }

    // Check declared game commands
    if let Some(command) = self.declared.get(&command_name) {
      if let Some((_, handler)) = &command.handler {
        return handler(&args, ctx);
      } else {
        // Command is declared but has no active handler (noop)
        ctx.console.print_system(format!(
          "Command '{}' has no effect in the current context",
          command_name
        ));
        ctx
          .console
          .print_system(format!("  {}", command.description));
        return CommandResult::Ok;
      }
    }

    // Unknown command
    ctx
      .console
      .print_error(format!("Unknown command: '{}'", command_name));
    ctx
      .console
      .print_system("Type 'help' for available commands");
    CommandResult::Error(format!("Unknown command: {}", command_name))
  }

  /// Get command info list for help display
  ///
  /// Includes both built-in and declared commands.
  pub fn command_info_list(&self) -> Vec<CommandInfo> {
    let mut list: Vec<CommandInfo> = self
      .builtins
      .values()
      .map(|cmd| CommandInfo {
        name: cmd.name.to_string(),
        description: cmd.description.to_string(),
        verbose_description: None,
        is_active: true,
      })
      .collect();

    list.extend(self.declared.values().map(|cmd| CommandInfo {
      name: cmd.name.clone(),
      description: cmd.description.clone(),
      verbose_description: cmd.verbose_description.clone(),
      is_active: cmd.handler.is_some(),
    }));

    list
  }

  /// Get detailed info for a specific command
  pub fn get_command_info(&self, name: &str) -> Option<CommandInfo> {
    if let Some(cmd) = self.builtins.get(name) {
      return Some(CommandInfo {
        name: cmd.name.to_string(),
        description: cmd.description.to_string(),
        verbose_description: None,
        is_active: true,
      });
    }

    if let Some(cmd) = self.declared.get(name) {
      return Some(CommandInfo {
        name: cmd.name.clone(),
        description: cmd.description.clone(),
        verbose_description: cmd.verbose_description.clone(),
        is_active: cmd.handler.is_some(),
      });
    }

    None
  }

  /// Check if a command exists (built-in or declared)
  pub fn has_command(&self, name: &str) -> bool {
    self.builtins.contains_key(name) || self.declared.contains_key(name)
  }

  /// Get total number of commands
  pub fn len(&self) -> usize {
    self.builtins.len() + self.declared.len()
  }

  /// Check if the registry is empty
  pub fn is_empty(&self) -> bool {
    self.builtins.is_empty() && self.declared.is_empty()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Create a minimal test CommandContext
  fn make_test_context<'a>(
    console: &'a mut ConsoleState,
    debug_config: &'a mut DebugConfig,
    time_scale: &'a mut f32,
  ) -> CommandContext<'a> {
    CommandContext {
      console,
      debug_config,
      quit_requested: false,
      capture_requested: None,
      time_scale,
      commands: Vec::new(),
      game_name: None,
      game_version: None,
    }
  }

  #[test]
  fn test_registry_new_is_empty() {
    let registry = CommandRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
  }

  #[test]
  fn test_register_builtin_command() {
    let mut registry = CommandRegistry::new();
    registry.register("test", "A test command", |_args, _ctx| CommandResult::Ok);

    assert!(!registry.is_empty());
    assert_eq!(registry.len(), 1);
    assert!(registry.has_command("test"));
    assert!(!registry.has_command("nonexistent"));
  }

  #[test]
  fn test_declare_game_command() {
    let mut registry = CommandRegistry::new();
    registry.declare("game:test", "Test game command", None::<&str>);

    assert_eq!(registry.len(), 1);
    assert!(registry.has_command("game:test"));

    let info = registry.get_command_info("game:test").unwrap();
    assert_eq!(info.name, "game:test");
    assert_eq!(info.description, "Test game command");
    assert!(!info.is_active); // No handler registered yet
    assert!(info.verbose_description.is_none());
  }

  #[test]
  fn test_declare_with_verbose_description() {
    let mut registry = CommandRegistry::new();
    registry.declare(
      "game:complex",
      "Brief description",
      Some("This is a much longer verbose description for the help command"),
    );

    let info = registry.get_command_info("game:complex").unwrap();
    assert_eq!(info.description, "Brief description");
    assert_eq!(
      info.verbose_description.as_deref(),
      Some("This is a much longer verbose description for the help command")
    );
  }

  #[test]
  fn test_register_handler() {
    let mut registry = CommandRegistry::new();
    registry.declare("game:action", "Do something", None::<&str>);

    // Command exists but is not active
    let info = registry.get_command_info("game:action").unwrap();
    assert!(!info.is_active);

    // Register handler
    let scope_id = 42;
    let result = registry.register_handler("game:action", scope_id, |_args, ctx| {
      ctx.console.print("Action executed!");
      CommandResult::Ok
    });
    assert!(result);

    // Now it should be active
    let info = registry.get_command_info("game:action").unwrap();
    assert!(info.is_active);
  }

  #[test]
  fn test_register_handler_for_undeclared_command() {
    let mut registry = CommandRegistry::new();

    // Try to register handler for non-existent command
    let result = registry.register_handler("game:nonexistent", 1, |_args, _ctx| CommandResult::Ok);
    assert!(!result);
  }

  #[test]
  fn test_unregister_handler() {
    let mut registry = CommandRegistry::new();
    registry.declare("game:toggle", "Toggle something", None::<&str>);
    registry.register_handler("game:toggle", 1, |_args, _ctx| CommandResult::Ok);

    // Verify active
    assert!(registry.get_command_info("game:toggle").unwrap().is_active);

    // Unregister
    registry.unregister_handler("game:toggle");

    // Verify inactive
    assert!(!registry.get_command_info("game:toggle").unwrap().is_active);
  }

  #[test]
  fn test_clear_scope() {
    let mut registry = CommandRegistry::new();

    // Declare multiple commands
    registry.declare("game:cmd1", "Command 1", None::<&str>);
    registry.declare("game:cmd2", "Command 2", None::<&str>);
    registry.declare("game:cmd3", "Command 3", None::<&str>);

    // Register handlers with different scopes
    let scene_a_scope = 100;
    let scene_b_scope = 200;

    registry.register_handler("game:cmd1", scene_a_scope, |_args, _ctx| CommandResult::Ok);
    registry.register_handler("game:cmd2", scene_a_scope, |_args, _ctx| CommandResult::Ok);
    registry.register_handler("game:cmd3", scene_b_scope, |_args, _ctx| CommandResult::Ok);

    // All should be active
    assert!(registry.get_command_info("game:cmd1").unwrap().is_active);
    assert!(registry.get_command_info("game:cmd2").unwrap().is_active);
    assert!(registry.get_command_info("game:cmd3").unwrap().is_active);

    // Clear scene A scope
    registry.clear_scope(scene_a_scope);

    // cmd1 and cmd2 should be inactive, cmd3 should still be active
    assert!(!registry.get_command_info("game:cmd1").unwrap().is_active);
    assert!(!registry.get_command_info("game:cmd2").unwrap().is_active);
    assert!(registry.get_command_info("game:cmd3").unwrap().is_active);
  }

  #[test]
  fn test_execute_builtin_command() {
    let mut registry = CommandRegistry::new();
    registry.register("ping", "Returns pong", |_args, ctx| {
      ctx.console.print("pong");
      CommandResult::Ok
    });

    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    let result = registry.execute("ping", &mut ctx);
    assert_eq!(result, CommandResult::Ok);
  }

  #[test]
  fn test_execute_declared_command_with_handler() {
    let mut registry = CommandRegistry::new();
    registry.declare("game:greet", "Say hello", None::<&str>);
    registry.register_handler("game:greet", 1, |args, ctx| {
      let name = args.first().unwrap_or(&"World");
      ctx.console.print(format!("Hello, {}!", name));
      CommandResult::Ok
    });

    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    let result = registry.execute("game:greet Alice", &mut ctx);
    assert_eq!(result, CommandResult::Ok);
  }

  #[test]
  fn test_execute_declared_command_noop() {
    let mut registry = CommandRegistry::new();
    registry.declare("game:inactive", "An inactive command", None::<&str>);
    // No handler registered

    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    let result = registry.execute("game:inactive", &mut ctx);
    assert_eq!(result, CommandResult::Ok); // Noop returns Ok
  }

  #[test]
  fn test_execute_unknown_command() {
    let registry = CommandRegistry::new();

    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    let result = registry.execute("unknown_cmd", &mut ctx);
    assert!(matches!(result, CommandResult::Error(_)));
  }

  #[test]
  fn test_execute_empty_input() {
    let registry = CommandRegistry::new();

    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    let result = registry.execute("", &mut ctx);
    assert_eq!(result, CommandResult::Ok);
  }

  #[test]
  fn test_execute_whitespace_only_input() {
    let registry = CommandRegistry::new();

    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    let result = registry.execute("   ", &mut ctx);
    assert_eq!(result, CommandResult::Ok);
  }

  #[test]
  fn test_command_case_insensitivity() {
    let mut registry = CommandRegistry::new();
    registry.register("test", "Test command", |_args, _ctx| CommandResult::Ok);

    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    // Should work with different cases
    assert_eq!(registry.execute("TEST", &mut ctx), CommandResult::Ok);
    assert_eq!(registry.execute("Test", &mut ctx), CommandResult::Ok);
    assert_eq!(registry.execute("test", &mut ctx), CommandResult::Ok);
  }

  #[test]
  fn test_command_with_arguments() {
    let mut registry = CommandRegistry::new();
    registry.register("add", "Add two numbers", |args, ctx| {
      if args.len() != 2 {
        return CommandResult::Error("Usage: add <a> <b>".to_string());
      }
      let a: i32 = args[0].parse().unwrap_or(0);
      let b: i32 = args[1].parse().unwrap_or(0);
      ctx.console.print(format!("{}", a + b));
      CommandResult::Ok
    });

    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    let result = registry.execute("add 5 3", &mut ctx);
    assert_eq!(result, CommandResult::Ok);
  }

  #[test]
  fn test_command_info_list() {
    let mut registry = CommandRegistry::new();
    registry.register("builtin1", "Builtin 1", |_args, _ctx| CommandResult::Ok);
    registry.register("builtin2", "Builtin 2", |_args, _ctx| CommandResult::Ok);
    registry.declare("game:cmd1", "Game cmd 1", None::<&str>);
    registry.declare("game:cmd2", "Game cmd 2", None::<&str>);
    registry.register_handler("game:cmd1", 1, |_args, _ctx| CommandResult::Ok);

    let list = registry.command_info_list();
    assert_eq!(list.len(), 4);

    // Count active vs inactive
    let active_count = list.iter().filter(|c| c.is_active).count();
    let inactive_count = list.iter().filter(|c| !c.is_active).count();
    assert_eq!(active_count, 3); // 2 builtins + 1 game cmd with handler
    assert_eq!(inactive_count, 1); // 1 game cmd without handler
  }

  #[test]
  fn test_get_command_info_nonexistent() {
    let registry = CommandRegistry::new();
    assert!(registry.get_command_info("nonexistent").is_none());
  }

  #[test]
  fn test_command_result_equality() {
    assert_eq!(CommandResult::Ok, CommandResult::Ok);
    assert_eq!(CommandResult::Quit, CommandResult::Quit);
    assert_eq!(
      CommandResult::Error("test".to_string()),
      CommandResult::Error("test".to_string())
    );
    assert_ne!(CommandResult::Ok, CommandResult::Quit);
    assert_ne!(
      CommandResult::Error("a".to_string()),
      CommandResult::Error("b".to_string())
    );
  }

  // Integration tests for the full command registration pipeline

  #[test]
  fn test_full_lifecycle_declare_register_execute_clear() {
    let mut registry = CommandRegistry::new();

    // Phase 1: Declare commands at "game startup"
    registry.declare(
      "game:reset",
      "Reset game state",
      Some("Resets all game state including score and enemies"),
    );
    registry.declare("game:spawn", "Spawn an entity", None::<&str>);

    // Verify commands exist but are inactive
    assert!(registry.has_command("game:reset"));
    assert!(registry.has_command("game:spawn"));
    assert!(!registry.get_command_info("game:reset").unwrap().is_active);
    assert!(!registry.get_command_info("game:spawn").unwrap().is_active);

    // Phase 2: Scene enters, registers handlers
    let scene_scope = 1;
    registry.register_handler("game:reset", scene_scope, |_args, ctx| {
      ctx.console.print("Game reset!");
      CommandResult::Ok
    });
    registry.register_handler("game:spawn", scene_scope, |_args, ctx| {
      ctx.console.print("Entity spawned!");
      CommandResult::Ok
    });

    // Verify commands are now active
    assert!(registry.get_command_info("game:reset").unwrap().is_active);
    assert!(registry.get_command_info("game:spawn").unwrap().is_active);

    // Phase 3: Execute commands
    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    assert_eq!(registry.execute("game:reset", &mut ctx), CommandResult::Ok);
    assert_eq!(
      registry.execute("game:spawn enemy", &mut ctx),
      CommandResult::Ok
    );

    // Phase 4: Scene exits, clear scope
    registry.clear_scope(scene_scope);

    // Verify commands are back to inactive (noop)
    assert!(!registry.get_command_info("game:reset").unwrap().is_active);
    assert!(!registry.get_command_info("game:spawn").unwrap().is_active);

    // Commands still exist, just noop
    assert!(registry.has_command("game:reset"));
    assert!(registry.has_command("game:spawn"));

    // Executing now returns Ok but prints noop message
    let result = registry.execute("game:reset", &mut ctx);
    assert_eq!(result, CommandResult::Ok);
  }

  #[test]
  fn test_multiple_scenes_with_different_scopes() {
    let mut registry = CommandRegistry::new();

    // Declare a command that works across scenes
    registry.declare("game:pause", "Pause the game", None::<&str>);

    // Scene A registers handler
    let scene_a = 100;
    registry.register_handler("game:pause", scene_a, |_args, ctx| {
      ctx.console.print("Paused in Scene A");
      CommandResult::Ok
    });

    assert!(registry.get_command_info("game:pause").unwrap().is_active);

    // Scene A exits
    registry.clear_scope(scene_a);
    assert!(!registry.get_command_info("game:pause").unwrap().is_active);

    // Scene B registers its own handler
    let scene_b = 200;
    registry.register_handler("game:pause", scene_b, |_args, ctx| {
      ctx.console.print("Paused in Scene B");
      CommandResult::Ok
    });

    assert!(registry.get_command_info("game:pause").unwrap().is_active);

    // Scene B exits
    registry.clear_scope(scene_b);
    assert!(!registry.get_command_info("game:pause").unwrap().is_active);
  }

  #[test]
  fn test_builtin_commands_not_affected_by_clear_scope() {
    let mut registry = CommandRegistry::new();

    // Register a builtin command
    registry.register("help", "Show help", |_args, _ctx| CommandResult::Ok);

    // Declare and register a game command
    let scope = 1;
    registry.declare("game:test", "Test", None::<&str>);
    registry.register_handler("game:test", scope, |_args, _ctx| CommandResult::Ok);

    // Both active
    assert!(registry.get_command_info("help").unwrap().is_active);
    assert!(registry.get_command_info("game:test").unwrap().is_active);

    // Clear scope
    registry.clear_scope(scope);

    // Builtin still active, game command inactive
    assert!(registry.get_command_info("help").unwrap().is_active);
    assert!(!registry.get_command_info("game:test").unwrap().is_active);
  }

  #[test]
  fn test_handler_replacement() {
    let mut registry = CommandRegistry::new();
    registry.declare("game:action", "Do action", None::<&str>);

    // First handler in scope 1
    let scope1 = 1;
    registry.register_handler("game:action", scope1, |_args, ctx| {
      ctx.console.print("Handler 1");
      CommandResult::Ok
    });

    // Replace with handler in scope 2
    let scope2 = 2;
    registry.register_handler("game:action", scope2, |_args, ctx| {
      ctx.console.print("Handler 2");
      CommandResult::Ok
    });

    // Command is still active
    assert!(registry.get_command_info("game:action").unwrap().is_active);

    // Clearing scope 1 should have no effect (handler was replaced)
    registry.clear_scope(scope1);
    assert!(registry.get_command_info("game:action").unwrap().is_active);

    // Clearing scope 2 should deactivate it
    registry.clear_scope(scope2);
    assert!(!registry.get_command_info("game:action").unwrap().is_active);
  }

  #[test]
  fn test_remove_builtin_existing() {
    let mut registry = CommandRegistry::new();
    registry.register("test:cmd", "A test command", |_args, _ctx| {
      CommandResult::Ok
    });

    assert!(registry.has_command("test:cmd"));
    assert!(registry.remove_builtin("test:cmd"));
    assert!(!registry.has_command("test:cmd"));
    assert!(registry.get_command_info("test:cmd").is_none());
  }

  #[test]
  fn test_remove_builtin_nonexistent() {
    let mut registry = CommandRegistry::new();
    assert!(!registry.remove_builtin("nonexistent"));
  }

  #[test]
  fn test_remove_builtin_does_not_affect_declared() {
    let mut registry = CommandRegistry::new();
    registry.declare("game:test", "A game command", None::<&str>);
    registry.register_handler("game:test", 1, |_args, _ctx| CommandResult::Ok);

    // remove_builtin should not remove declared commands
    assert!(!registry.remove_builtin("game:test"));
    assert!(registry.has_command("game:test"));
    assert!(registry.get_command_info("game:test").unwrap().is_active);
  }

  #[test]
  fn test_remove_builtin_then_redeclare_as_game_command() {
    let mut registry = CommandRegistry::new();
    registry.register("system:test", "Built-in test", |_args, _ctx| {
      CommandResult::Ok
    });

    // Remove the builtin
    assert!(registry.remove_builtin("system:test"));
    assert!(!registry.has_command("system:test"));

    // Re-declare as a game command with custom behavior
    registry.declare("system:test", "Overridden test command", None::<&str>);
    registry.register_handler("system:test", 1, |_args, ctx| {
      ctx.console.print("Custom handler!");
      CommandResult::Ok
    });

    assert!(registry.has_command("system:test"));
    let info = registry.get_command_info("system:test").unwrap();
    assert_eq!(info.description, "Overridden test command");
    assert!(info.is_active);
  }

  #[test]
  fn test_remove_builtin_reduces_count() {
    let mut registry = CommandRegistry::new();
    registry.register("cmd1", "Command 1", |_args, _ctx| CommandResult::Ok);
    registry.register("cmd2", "Command 2", |_args, _ctx| CommandResult::Ok);
    assert_eq!(registry.len(), 2);

    registry.remove_builtin("cmd1");
    assert_eq!(registry.len(), 1);
    assert!(!registry.has_command("cmd1"));
    assert!(registry.has_command("cmd2"));
  }

  #[test]
  fn test_execute_removed_builtin_returns_error() {
    let mut registry = CommandRegistry::new();
    registry.register("test", "Test command", |_args, _ctx| CommandResult::Ok);
    registry.remove_builtin("test");

    let mut console = ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    let result = registry.execute("test", &mut ctx);
    assert!(matches!(result, CommandResult::Error(_)));
  }
}
