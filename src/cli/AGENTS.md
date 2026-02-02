# CLI Module - AGENTS.md

## Overview

The `cli` module provides the `ryo2d` command-line interface for managing Ryo2D game projects. It handles building, running, and developing games with a simple, ergonomic interface.

## Commands

| Command | Description |
|---------|-------------|
| `ryo2d init` | Create a new Ryo2D project |
| `ryo2d build` | Build a production release of the game |
| `ryo2d run` | Run the production build (warns if no build exists) |
| `ryo2d dev` | Development mode: build + run with file watching |
| `ryo2d install-skills` | Install Agent Skills for AI assistants |

## Architecture

```
cli/
├── mod.rs              # Module exports and CLI entry point
├── commands/
│   ├── mod.rs          # Command registry
│   ├── init.rs         # Create new project
│   ├── build.rs        # Production build command
│   ├── run.rs          # Run production build
│   ├── dev.rs          # Development mode with watcher
│   └── install_skills.rs  # Install Agent Skills
└── AGENTS.md           # This file
```

## Design Principles

### 1. Workspace-Aware

All commands operate relative to the current workspace. The CLI uses `Workspace::discover()` to find the project root (via `Ryo.toml`).

### 2. Clear Feedback

- Commands provide clear progress output
- Errors are descriptive and actionable
- Warnings (e.g., "no build exists") help users understand state

### 3. Cargo Integration

The CLI wraps Cargo for Rust compilation:
- `ryo2d build` → `cargo build --release`
- `ryo2d dev` → `cargo run` (debug build with hot reload capability)
- `ryo2d run` → Executes the built binary directly

### 4. Future Extensibility

The command structure supports future additions:
- `ryo2d new` - Create new project from template
- `ryo2d assets` - Asset pipeline commands
- `ryo2d package` - Distribution packaging

## Implementation Notes

### Entry Point

The CLI can be invoked via:
1. **Binary crate**: A separate `src/bin/ryo2d.rs` that uses the cli module
2. **Cargo install**: `cargo install ryo2d` provides the `ryo2d` command

### Error Handling

CLI commands return `Result<(), CliError>` where `CliError` provides:
- User-friendly error messages
- Appropriate exit codes
- Suggestions for resolution

### Output Style

Follow these conventions:
- Use colored output when terminal supports it
- Prefix actions: `Building...`, `Running...`, `Watching...`
- Success: green checkmark or "Done"
- Errors: red with clear message
- Warnings: yellow with context

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | Argument parsing and command structure |
| `notify` | File system watching for dev mode |
| `colored` | Terminal colors (optional) |

## Notes for AI Agents

1. Keep commands simple and focused
2. Provide helpful error messages with suggestions
3. Use the workspace system for path resolution
4. Don't reinvent Cargo - wrap it appropriately
5. Consider cross-platform compatibility (Windows, macOS, Linux)
