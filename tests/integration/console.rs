//! Console command integration tests
//!
//! Tests for console command registration, removal, and override flows.
//! These verify that built-in commands can be removed and replaced at
//! the engine, game, and scene levels.

use ryo2d::prelude::*;

// ============================================================================
// Helper
// ============================================================================

fn make_registry_with_builtins() -> CommandRegistry {
  let mut registry = CommandRegistry::new();
  ryo2d::core::console::register_builtins(&mut registry);
  registry
}

fn make_context<'a>(console: &'a mut ConsoleState, time_scale: &'a mut f32) -> CommandContext<'a> {
  use ryo2d::core::config::DebugConfig;
  CommandContext {
    console,
    debug_config: Box::leak(Box::new(DebugConfig::default())),
    quit_requested: false,
    time_scale,
    commands: Vec::new(),
    game_name: None,
    game_version: None,
  }
}

// ============================================================================
// Engine-level: with_console_commands on EngineBuilder
// ============================================================================

/// Verifies that builtins exist after standard registration.
#[test]
fn test_builtins_include_ip_commands() {
  let registry = make_registry_with_builtins();
  assert!(registry.has_command("system:ip:lan"));
  assert!(registry.has_command("system:ip:wan"));
  assert!(registry.has_command("system:version"));
  assert!(registry.has_command("system:timescale"));
}

/// Simulates the EngineBuilder flow: register builtins then remove specific ones.
#[test]
fn test_engine_builder_remove_builtin_flow() {
  let mut registry = CommandRegistry::new();
  ryo2d::core::console::register_builtins(&mut registry);

  // Simulate EngineBuilder.with_console_commands(|cmd| cmd.remove_builtin("system:ip:wan"))
  let commands_to_remove = vec!["system:ip:wan", "system:version"];
  for name in &commands_to_remove {
    registry.remove_builtin(name);
  }

  // Removed commands are gone
  assert!(!registry.has_command("system:ip:wan"));
  assert!(!registry.has_command("system:version"));

  // Other commands still work
  assert!(registry.has_command("system:ip:lan"));
  assert!(registry.has_command("system:timescale"));
  assert!(registry.has_command("help"));
  assert!(registry.has_command("debug:fps"));
}

/// Executing a removed builtin returns an error (unknown command).
#[test]
fn test_removed_builtin_is_unknown() {
  let mut registry = make_registry_with_builtins();
  registry.remove_builtin("system:ip:lan");

  let mut console = ConsoleState::new();
  let mut time_scale = 1.0;
  let mut ctx = make_context(&mut console, &mut time_scale);

  let result = registry.execute("system:ip:lan", &mut ctx);
  assert!(matches!(result, CommandResult::Error(_)));
}

// ============================================================================
// Game/Scene-level: remove_builtin then re-declare as game command
// ============================================================================

/// Simulates a game removing a builtin in init() and replacing it with
/// a custom game command.
#[test]
fn test_game_context_remove_and_override_builtin() {
  let mut registry = make_registry_with_builtins();

  // Phase 1: Game init — remove builtin, re-declare as game command
  // (GameContext.remove_builtin_console_command delegates to this)
  assert!(registry.remove_builtin("system:version"));
  registry.declare(
    "system:version",
    "Custom version info",
    Some("Shows custom game version information"),
  );

  // Should exist but be inactive (no handler yet)
  let info = registry.get_command_info("system:version").unwrap();
  assert_eq!(info.description, "Custom version info");
  assert!(!info.is_active);

  // Phase 2: Register custom handler (scope_id 0 = game-level, permanent)
  registry.register_handler("system:version", 0, |_args, ctx| {
    ctx.console.print("MyGame v2.0");
    CommandResult::Ok
  });

  let info = registry.get_command_info("system:version").unwrap();
  assert!(info.is_active);

  // Phase 3: Execute — should use custom handler
  let mut console = ConsoleState::new();
  let mut time_scale = 1.0;
  let mut ctx = make_context(&mut console, &mut time_scale);

  let result = registry.execute("system:version", &mut ctx);
  assert_eq!(result, CommandResult::Ok);
}

/// Simulates a scene removing a builtin and providing a scene-scoped override.
#[test]
fn test_scene_context_remove_and_override_builtin() {
  let mut registry = make_registry_with_builtins();

  // Scene removes the builtin and declares a replacement
  // (SceneContext.remove_builtin_console_command delegates to this)
  assert!(registry.remove_builtin("debug:fps"));
  registry.declare("debug:fps", "Custom FPS toggle", None::<&str>);

  let scene_scope = 42u64;
  registry.register_handler("debug:fps", scene_scope, |_args, ctx| {
    ctx.console.print("Custom FPS handler active");
    CommandResult::Ok
  });

  // Execute the custom handler
  let mut console = ConsoleState::new();
  let mut time_scale = 1.0;
  let mut ctx = make_context(&mut console, &mut time_scale);

  let result = registry.execute("debug:fps", &mut ctx);
  assert_eq!(result, CommandResult::Ok);

  // When scene exits, handler is cleaned up
  registry.clear_scope(scene_scope);
  let info = registry.get_command_info("debug:fps").unwrap();
  assert!(!info.is_active);

  // Executing now gives noop message (declared but inactive)
  let result = registry.execute("debug:fps", &mut ctx);
  assert_eq!(result, CommandResult::Ok); // noop returns Ok
}

/// Removing multiple builtins at once (simulating EngineBuilder with chained calls).
#[test]
fn test_remove_multiple_builtins_chained() {
  let mut registry = make_registry_with_builtins();

  let to_remove = ["system:ip:lan", "system:ip:wan", "system:timescale"];
  for name in &to_remove {
    assert!(registry.remove_builtin(name));
  }

  for name in &to_remove {
    assert!(!registry.has_command(name));
  }

  // Core commands still intact
  assert!(registry.has_command("help"));
  assert!(registry.has_command("quit"));
  assert!(registry.has_command("clear"));
  assert!(registry.has_command("system:version"));
}

/// Removing a builtin twice returns false the second time.
#[test]
fn test_remove_builtin_idempotent() {
  let mut registry = make_registry_with_builtins();

  assert!(registry.remove_builtin("system:ip:lan"));
  assert!(!registry.remove_builtin("system:ip:lan"));
}
