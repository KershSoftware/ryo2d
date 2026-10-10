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
ryo2d netplay          # Tools for testing multiplayer with friends
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

# With profiler window for performance analysis
ryo2d dev --profiler
# or shorthand:
ryo2d dev -p

# When ready for release testing
ryo2d build
ryo2d run
```

## Profiler Flag

`ryo2d dev --profiler` (or `-p`) enables the built-in profiler:

1. Compiles with `--features profiler`
2. Sets `RYO_PUFFIN_PROFILER=1` environment variable
3. Opens a second window showing real-time flamegraphs

Add `profile_function!()` to your update/render to see data:

```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    profile_function!();
    // ... your code
}
```

The profiler window shows:
- FPS and frame work time
- Color-coded scope bars (hover for details)
- Setup hints if no scopes detected

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

## Netplay Testing

Test multiplayer with friends over the internet without manual server setup:

```bash
ryo2d netplay start             # Start temp signaling server
ryo2d netplay start --room xyz  # Custom room name
ryo2d netplay ip                # Show your LAN and WAN IPs
```

`netplay start` does the following:
1. Installs `matchbox_server` (if needed)
2. Installs `cloudflared` (if needed, prompts Y/n)
3. Starts local matchbox signaling server
4. Creates Cloudflare Tunnel for internet access
5. Displays connection info and code snippet

The tunnel URL is **temporary** - it changes each run and stops when you Ctrl+C.

For production games, deploy your own signaling server (see `docs/matchbox-server-deploy.md`).

## Tips

- Run CLI from project root (where `Ryo.toml` is)
- Use `ryo2d dev` for fast iteration
- Asset hot reload works in dev mode (no recompile needed)
- For engine dev, use `./run` scripts from repo root
- Use `ryo2d netplay` for quick multiplayer testing

## Frame Capture

Save frames as PNGs to check rendering without watching the window. Works with `ryo2d dev`, `ryo2d run`, `./run` and `cargo run` (the game inherits the environment):

| Variable | Meaning |
|----------|---------|
| `RYO_CAPTURE_FRAMES=60,120` | Presented-frame numbers to capture |
| `RYO_CAPTURE_DIR=captures` | Output directory (default `captures/`) |
| `RYO_CAPTURE_EXIT=1` | Quit after the last capture; exit code 1 if one failed |

Files are named `frame-000060.png` at game resolution (no letterboxing). In dev builds, F12 or the console command `debug:capture [path]` saves the next frame.

```bash
RYO_CAPTURE_FRAMES=120 RYO_CAPTURE_EXIT=1 ryo2d dev
```
