---
name: ryo-cli
description: Using the ryo2d CLI for building, running, and developing games.
metadata:
  version: 1.0.0
---

# Ryo2D CLI Specialist

You help users use the `ryo2d` CLI for managing game projects.

## Commands

Run from a game project directory (containing `Ryo.toml`):

```bash
ryo2d init <name>      # Create new project
ryo2d build            # Production build (cargo build --release)
ryo2d run              # Run production build
ryo2d dev              # Development mode with hot reload
ryo2d export           # Package release into distributable zip
ryo2d install-skills   # Add Agent Skills to existing project
```

## Project Structure

```
my_game/
├── Ryo.toml           # Project configuration
├── Cargo.toml         # Rust dependencies
├── src/
│   └── main.rs
└── assets/
    ├── sprites/
    ├── sounds/
    └── music/
```

## Ryo.toml

```toml
[project]
name = "My Game"
version = "0.1.0"

[window]
title = "My Game"
width = 1280
height = 720

[physics]
hz = 120

[debug]
show_fps = true
```

## Development Workflow

```bash
cd my_game
ryo2d dev          # Fast iteration with hot reload

# When ready for release testing
ryo2d build
ryo2d run
```

## Engine Development

When working ON the engine (not a game), use root scripts:

```powershell
# Windows (from repo root)
./run              # Interactive example selection
./run breakout     # Run specific example
./run check        # Fast cargo check
./run lint         # Clippy
./run test         # Run tests
```

```bash
# Unix/macOS
./run.sh breakout
./run.sh test
```

| Context | Tool |
|---------|------|
| Making a game | `ryo2d build/run/dev` |
| Working on engine | `./run` scripts or `cargo` |
| Testing with examples | `./run <example>` |

## Configuration Priority

Settings cascade (highest to lowest priority):

1. `system.json` (user preferences)
2. `Ryo.toml` (project defaults)
3. Engine defaults

## Dev Build Performance

For playable performance in dev builds, ensure `Cargo.toml` includes:

```toml
# Optimize dependencies in dev builds for playable performance
[profile.dev.package."*"]
opt-level = 2
```

This optimizes **dependencies** (physics, rendering) while keeping game code unoptimized for fast compilation. Without it, dev builds run extremely slowly.

- New projects (`ryo2d init`) include this automatically
- `ryo2d dev` warns and offers to add it if missing
- First build after adding is slower (recompiling deps), then fast

## Tips

- Run CLI from project root (where `Ryo.toml` is)
- Use `ryo2d dev` for fast iteration
- Asset hot reload works in dev mode (no recompile needed)
- For engine dev, use `./run` scripts from repo root

