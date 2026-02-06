//! Built-in console commands
//!
//! Commands use a category:command format (e.g., `debug:fps`, `system:timescale`).
//! Root-level commands (help, clear, quit, exit) don't require a category.

use std::net::UdpSocket;

use super::commands::{CommandRegistry, CommandResult};

/// Register all built-in commands with the registry
pub fn register_builtins(registry: &mut CommandRegistry) {
  // ═══════════════════════════════════════════════════════════════════════════
  // Root-level commands (no category required)
  // ═══════════════════════════════════════════════════════════════════════════

  // Helper function to list commands by category
  fn list_category_commands(ctx: &mut super::commands::CommandContext, prefix: &str) {
    let mut commands: Vec<_> = ctx
      .commands
      .iter()
      .filter(|cmd| cmd.name.starts_with(prefix))
      .collect();

    if commands.is_empty() {
      ctx.console.print_system(format!(
        "No {} commands available",
        prefix.trim_end_matches(':')
      ));
      return;
    }

    commands.sort_by(|a, b| a.name.cmp(&b.name));
    ctx
      .console
      .print_system(format!("{} commands:", prefix.trim_end_matches(':')));
    ctx.console.print_system("");
    for cmd in commands {
      let padded_name = format!("{:<20}", cmd.name);
      // Show inactive status for game commands
      let status = if !cmd.is_active { " (inactive)" } else { "" };
      ctx
        .console
        .print(format!("  {} - {}{}", padded_name, cmd.description, status));
    }
  }

  // Help command
  registry.register(
    "help",
    "List available commands or show help for a specific command",
    |args, ctx| {
      if let Some(cmd_name) = args.first() {
        // Look up specific command
        if let Some(cmd) = ctx.commands.iter().find(|c| c.name == *cmd_name) {
          ctx.console.print_system(format!("Command: {}", cmd.name));
          // Show verbose description if available, otherwise show regular description
          if let Some(verbose) = &cmd.verbose_description {
            ctx.console.print(format!("  {}", verbose));
          } else {
            ctx.console.print(format!("  {}", cmd.description));
          }
          // Show active/inactive status for game commands
          if cmd.name.starts_with("game:") {
            if cmd.is_active {
              ctx.console.print_system("  Status: Active");
            } else {
              ctx
                .console
                .print_system("  Status: Inactive (no effect in current context)");
            }
          }
        } else {
          ctx
            .console
            .print_error(format!("Unknown command: '{}'", cmd_name));
          ctx
            .console
            .print_system("Type 'help' for available commands");
        }
      } else {
        ctx.console.print_system("Available commands:");
        ctx.console.print_system("");
        ctx
          .console
          .print("  help [cmd]          - List commands or show specific help");
        ctx
          .console
          .print("  clear               - Clear console output");
        ctx.console.print("  quit / exit         - Exit the game");
        ctx.console.print_system("");
        ctx.console.print_system("debug:");
        ctx
          .console
          .print("  debug:fps           - Toggle FPS display (F1)");
        ctx
          .console
          .print("  debug:stats         - Toggle debug statistics overlay (F1)");
        ctx
          .console
          .print("  debug:timing        - Toggle frame timing breakdown (F1)");
        ctx
          .console
          .print("  debug:colliders     - Toggle collision visualization (F2)");
        ctx
          .console
          .print("  debug:velocities    - Toggle velocity vectors (F3)");
        ctx
          .console
          .print("  debug:origins       - Toggle entity origin points (F4)");
        ctx.console.print_system("");
        ctx.console.print_system("system:");
        ctx
          .console
          .print("  system:timescale <n> - Set game time scale (1.0 = normal)");
        ctx
          .console
          .print("  system:version       - Show engine version information");
        ctx
          .console
          .print("  system:ip:lan        - Show local LAN IP address");
        ctx
          .console
          .print("  system:ip:wan        - Show public WAN IP address");

        // Dynamically list game: commands
        let mut game_commands: Vec<_> = ctx
          .commands
          .iter()
          .filter(|cmd| cmd.name.starts_with("game:"))
          .collect();

        if !game_commands.is_empty() {
          game_commands.sort_by(|a, b| a.name.cmp(&b.name));
          ctx.console.print_system("");
          ctx.console.print_system("game:");
          for cmd in game_commands {
            // Format: pad name to 18 chars, then description, with inactive indicator
            let padded_name = format!("{:<18}", cmd.name);
            let status = if cmd.is_active { "" } else { " (inactive)" };
            ctx
              .console
              .print(format!("  {} - {}{}", padded_name, cmd.description, status));
          }
        }

        ctx.console.print_system("");
        ctx
          .console
          .print_system("Type 'debug', 'system', or 'game' to list category commands");
        ctx
          .console
          .print_system("Press ~ (tilde) to toggle console");
        ctx
          .console
          .print_system("Press Escape or X to close console");
        ctx
          .console
          .print_system("Use mouse wheel or Page Up/Down to scroll");
      }
      CommandResult::Ok
    },
  );

  // Category listing commands
  registry.register("debug", "List all debug commands", |_args, ctx| {
    list_category_commands(ctx, "debug:");
    CommandResult::Ok
  });

  registry.register("system", "List all system commands", |_args, ctx| {
    list_category_commands(ctx, "system:");
    CommandResult::Ok
  });

  registry.register("game", "List all game-specific commands", |_args, ctx| {
    list_category_commands(ctx, "game:");
    CommandResult::Ok
  });

  // Clear command
  registry.register("clear", "Clear console output", |_args, ctx| {
    ctx.console.clear();
    CommandResult::Ok
  });

  // Quit commands
  registry.register("quit", "Exit the game", |_args, ctx| {
    ctx.quit_requested = true;
    ctx.console.print_system("Shutting down...");
    CommandResult::Quit
  });

  registry.register("exit", "Exit the game (alias for quit)", |_args, ctx| {
    ctx.quit_requested = true;
    ctx.console.print_system("Shutting down...");
    CommandResult::Quit
  });

  // ═══════════════════════════════════════════════════════════════════════════
  // debug: category - Debug visualization commands
  // ═══════════════════════════════════════════════════════════════════════════

  // FPS toggle (F1 cycles through overlays)
  registry.register("debug:fps", "Toggle FPS display", |_args, ctx| {
    ctx.debug_config.show_fps = !ctx.debug_config.show_fps;
    if ctx.debug_config.show_fps {
      ctx.console.print("FPS display enabled");
    } else {
      ctx.console.print("FPS display disabled");
    }
    CommandResult::Ok
  });

  // Stats command - show current debug stats (F1 cycles through overlays)
  registry.register(
    "debug:stats",
    "Toggle debug statistics overlay",
    |_args, ctx| {
      ctx.debug_config.show_stats = !ctx.debug_config.show_stats;
      if ctx.debug_config.show_stats {
        ctx.console.print("Debug statistics visible");
      } else {
        ctx.console.print("Debug statistics hidden");
      }
      CommandResult::Ok
    },
  );

  // Timing command - toggle frame timing breakdown (F1 cycles through overlays)
  registry.register(
    "debug:timing",
    "Toggle frame timing breakdown overlay",
    |_args, ctx| {
      ctx.debug_config.show_frame_timing = !ctx.debug_config.show_frame_timing;
      if ctx.debug_config.show_frame_timing {
        ctx.console.print("Frame timing breakdown visible");
      } else {
        ctx.console.print("Frame timing breakdown hidden");
      }
      CommandResult::Ok
    },
  );

  // Colliders toggle (F2)
  registry.register(
    "debug:colliders",
    "Toggle collision boundary visualization",
    |_args, ctx| {
      ctx.debug_config.show_colliders = !ctx.debug_config.show_colliders;
      if ctx.debug_config.show_colliders {
        ctx.console.print("Collision boundaries visible");
      } else {
        ctx.console.print("Collision boundaries hidden");
      }
      CommandResult::Ok
    },
  );

  // Velocities toggle (F3)
  registry.register(
    "debug:velocities",
    "Toggle velocity vector visualization",
    |_args, ctx| {
      ctx.debug_config.show_velocities = !ctx.debug_config.show_velocities;
      if ctx.debug_config.show_velocities {
        ctx.console.print("Velocity vectors visible");
      } else {
        ctx.console.print("Velocity vectors hidden");
      }
      CommandResult::Ok
    },
  );

  // Origins toggle (F4)
  registry.register(
    "debug:origins",
    "Toggle entity origin point visualization",
    |_args, ctx| {
      ctx.debug_config.show_origins = !ctx.debug_config.show_origins;
      if ctx.debug_config.show_origins {
        ctx.console.print("Entity origins visible");
      } else {
        ctx.console.print("Entity origins hidden");
      }
      CommandResult::Ok
    },
  );

  // ═══════════════════════════════════════════════════════════════════════════
  // system: category - System/engine commands
  // ═══════════════════════════════════════════════════════════════════════════

  // Time scale command
  registry.register(
    "system:timescale",
    "Set game time scale (1.0 = normal speed)",
    |args, ctx| {
      if args.is_empty() {
        ctx
          .console
          .print(format!("Current time scale: {:.2}", *ctx.time_scale));
        return CommandResult::Ok;
      }

      match args[0].parse::<f32>() {
        Ok(scale) => {
          if scale < 0.0 {
            ctx.console.print_error("Time scale cannot be negative");
            return CommandResult::Error("Invalid time scale".to_string());
          }
          if scale > 10.0 {
            ctx.console.print_error("Time scale cannot exceed 10.0");
            return CommandResult::Error("Invalid time scale".to_string());
          }
          *ctx.time_scale = scale;
          ctx.console.print(format!("Time scale set to {:.2}", scale));
          CommandResult::Ok
        }
        Err(_) => {
          ctx
            .console
            .print_error(format!("Invalid number: '{}'", args[0]));
          ctx.console.print_system("Usage: system:timescale <number>");
          CommandResult::Error("Invalid argument".to_string())
        }
      }
    },
  );

  // Version/info command
  registry.register(
    "system:version",
    "Show engine and game version information",
    |_args, ctx| {
      // Show game info if available
      if let Some(name) = &ctx.game_name {
        ctx.console.print(name.to_string());
        if let Some(version) = &ctx.game_version {
          ctx.console.print(format!("  Version: {}", version));
        }
        ctx.console.print("");
      }
      ctx.console.print("Ryo2D Game Engine");
      ctx
        .console
        .print(format!("  Engine: {}", env!("CARGO_PKG_VERSION")));
      ctx.console.print(format!(
        "  Build:  {}",
        if cfg!(debug_assertions) {
          "Dev"
        } else {
          "Release"
        }
      ));
      CommandResult::Ok
    },
  );

  // LAN IP command
  registry.register(
    "system:ip:lan",
    "Show local LAN IP address",
    |_args, ctx| {
      match get_lan_ip() {
        Some(ip) => ctx.console.print(format!("LAN IP: {}", ip)),
        None => ctx.console.print_error("Could not determine LAN IP"),
      }
      CommandResult::Ok
    },
  );

  // WAN IP command
  registry.register(
    "system:ip:wan",
    "Show public WAN IP address",
    |_args, ctx| {
      ctx.console.print_system("Fetching WAN IP...");
      match get_wan_ip() {
        Some(ip) => ctx.console.print(format!("WAN IP: {}", ip)),
        None => ctx
          .console
          .print_error("Could not determine WAN IP (no internet?)"),
      }
      CommandResult::Ok
    },
  );
}

/// Get the local LAN IP by connecting a UDP socket to a public address
fn get_lan_ip() -> Option<String> {
  let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
  socket.connect("8.8.8.8:80").ok()?;
  let local_addr = socket.local_addr().ok()?;
  Some(local_addr.ip().to_string())
}

/// Get the public WAN IP via a plain HTTP request to api.ipify.org
fn get_wan_ip() -> Option<String> {
  use std::io::{Read, Write};
  use std::net::{TcpStream, ToSocketAddrs};
  use std::time::Duration;

  let timeout = Duration::from_secs(5);

  // Resolve via DNS then connect with timeout
  let addr = "api.ipify.org:80".to_socket_addrs().ok()?.next()?;
  let mut stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
  stream.set_read_timeout(Some(timeout)).ok()?;

  let request = "GET / HTTP/1.1\r\nHost: api.ipify.org\r\nConnection: close\r\n\r\n";
  stream.write_all(request.as_bytes()).ok()?;

  let mut response = String::new();
  stream.read_to_string(&mut response).ok()?;

  // Parse IP from HTTP response body (after headers)
  let body = response.split("\r\n\r\n").nth(1)?;
  let ip = body.trim().to_string();

  // Basic validation: should look like an IP address
  if !ip.is_empty() && ip.len() <= 45 {
    Some(ip)
  } else {
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::config::DebugConfig;

  fn make_test_context<'a>(
    console: &'a mut super::super::ConsoleState,
    debug_config: &'a mut DebugConfig,
    time_scale: &'a mut f32,
  ) -> super::super::commands::CommandContext<'a> {
    super::super::commands::CommandContext {
      console,
      debug_config,
      quit_requested: false,
      time_scale,
      commands: Vec::new(),
      game_name: None,
      game_version: None,
    }
  }

  #[test]
  fn test_builtins_register_ip_commands() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);

    assert!(registry.has_command("system:ip:lan"));
    assert!(registry.has_command("system:ip:wan"));
  }

  #[test]
  fn test_ip_lan_command_executes() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);

    let mut console = super::super::ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    // Should always return Ok (prints error message on failure, doesn't return Error)
    let result = registry.execute("system:ip:lan", &mut ctx);
    assert_eq!(result, super::super::commands::CommandResult::Ok);
  }

  #[test]
  fn test_ip_wan_command_executes() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);

    let mut console = super::super::ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);

    // Should always return Ok regardless of network availability
    let result = registry.execute("system:ip:wan", &mut ctx);
    assert_eq!(result, super::super::commands::CommandResult::Ok);
  }

  #[test]
  fn test_get_lan_ip_returns_valid_format() {
    // This test requires network access but should work in most environments
    if let Some(ip) = get_lan_ip() {
      // Should be a valid IPv4 address
      assert!(ip.contains('.'));
      assert!(ip.len() <= 15);
      // Should not be 0.0.0.0 or empty
      assert_ne!(ip, "0.0.0.0");
      assert!(!ip.is_empty());
    }
    // If None, it just means no network — that's fine for CI
  }

  #[test]
  fn test_ip_commands_can_be_removed() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);

    assert!(registry.has_command("system:ip:lan"));
    assert!(registry.remove_builtin("system:ip:lan"));
    assert!(!registry.has_command("system:ip:lan"));

    assert!(registry.has_command("system:ip:wan"));
    assert!(registry.remove_builtin("system:ip:wan"));
    assert!(!registry.has_command("system:ip:wan"));
  }
}
