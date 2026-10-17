---
name: ryo-cli
description: Using the ryo2d CLI for building, running, and developing games.
metadata:
  version: 1.1.0
---

# Ryo2D CLI Specialist

You help users use the `ryo2d` CLI for managing game projects.

## Commands

Run from a game project directory (containing `Ryo.toml`):

```bash
ryo2d init <name>      # Create new project
ryo2d build            # Production build (cargo build --release)
ryo2d run              # Run production build
ryo2d dev              # Development mode (debug build, asset hot reload)
ryo2d export           # Package release into distributable zip
ryo2d install-skills   # Add Agent Skills to existing project
ryo2d netplay          # Tools for testing multiplayer with friends
```

## Project Structure

```
my_game/
├── Ryo.toml           # Project configuration
├── Cargo.toml         # Rust dependencies
├── main.rs            # Game entry point
├── build.rs           # Build script (Windows icon embedding)
├── src/               # Additional modules (optional)
├── data/              # Save data (auto-generated at runtime)
└── assets/            # Game assets (sprites, sounds, music, ...)
```

## Ryo.toml

```toml
[project]
name = "my_game"
title = "My Game"
version = "0.1.0"

[system]
resolution = "1280x720"   # the game's canvas, scaled to fit the window (with pixel_art, the target)
# pixel_art = true         # render into the resolution, scaled by a whole number with bars

[debug]
show_colliders = false

[release]
# logo = "assets/icon.png"
# portable = false
# save_folder = "MyStudio/MyGame"
```

## Release bundle

`ryo2d build` writes `release/` with the game binary, `assets/`, `levels/`, `icon.png` and `licenses/` (plus `data/` when `[release] portable = true`). The game runs from that folder wherever it's launched from, and saves to the OS user data folder unless portable - see the `ryo-config-save` skill.

- **`licenses/` holds the licences of the fonts the engine builds into every game** (feature `default-fonts`): Inter and JetBrains Mono (SIL Open Font License 1.1, which asks that every copy of the fonts carries its notice and licence) and m5x7 (CC0), with a README naming them. Ship the folder with the game; `ryo2d export` always puts it in the zip. Add your own fonts' and assets' licences beside it.
- The `ryo2d` command has no web build: a game you build for the web yourself must ship the same licence files (copy `release/licenses/` from a `ryo2d build`).

## Development Workflow

```bash
cd my_game
ryo2d dev          # Fast iteration (debug build, asset hot reload)

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
2. Sets `RYO_PROFILER_AUTO_OPEN=1` so the profiler window opens at startup
3. Shows a second window with real-time flamegraphs

Add `profile_function!()` to your update/render to see data:

```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    profile_function!();
    // ... your code
    SceneAction::Continue
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
./dev              # Interactive example selection
./dev breakout     # Run specific example
./dev check        # Fast cargo check
./dev lint         # Clippy
./dev test         # Run tests
```

```bash
# Unix/macOS
./dev.sh breakout
./dev.sh test
./dev.sh web lighting   # serve a demo in the browser (WebGPU) with trunk
```

`./dev web <demo>` builds the demo for `wasm32-unknown-unknown` and serves it with trunk (`cargo install trunk --locked`), opening the browser; trunk rebuilds on engine and demo edits. Only demos with an `index.html` and `Trunk.toml` run on the web (`lighting` today). `check` and `lint` also build the engine for wasm32. The `ryo2d` command has no web build yet.

| Context | Tool |
|---------|------|
| Making a game | `ryo2d build/run/dev` |
| Working on engine | `./dev` scripts or `cargo` |
| Testing with examples | `./dev <example>` |

## Configuration Priority

Startup settings resolve in one order, each layer overriding only what it sets: engine defaults -> `Ryo.toml` (dev) or the values `ryo2d build` embedded from it (release) -> `EngineBuilder` calls -> saved player preferences in `system.json` (only with `with_user_preferences`).

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
ryo2d netplay ip                # Show your LAN and WAN IPs
```

`netplay start` does the following:
1. Installs `matchbox_server` (if needed)
2. Installs `cloudflared` (if needed, prompts Y/n)
3. Starts local matchbox signaling server
4. Creates Cloudflare Tunnel for internet access
5. Displays connection info and code snippet

The tunnel URL is **temporary** - it changes each run and stops when you Ctrl+C.

For production games, deploy your own signaling server (see `docs/architecture/matchbox-server.md` in the engine repo).

## Tips

- Run CLI from project root (where `Ryo.toml` is)
- Use `ryo2d dev` for fast iteration
- Asset hot reload works in dev mode (no recompile needed)
- For engine dev, use `./dev` scripts from repo root
- Use `ryo2d netplay` for quick multiplayer testing

## Frame Capture

Save frames as PNGs to check rendering without watching the window. Works with `ryo2d dev`, `ryo2d run`, `./dev` and `cargo run` (the game inherits the environment):

| Variable | Meaning |
|----------|---------|
| `RYO_CAPTURE_FRAMES=60,120` | Presented-frame numbers to capture |
| `RYO_CAPTURE_DIR=captures` | Output directory (default `captures/`) |
| `RYO_CAPTURE_EXIT=1` | Quit after the last capture; exit code 1 if one failed |
| `RYO_INPUT="60:Digit2 200-260:ArrowUp 300:Click@320x180"` | Dev builds: tap a key on a frame (`N:Key`), hold it from frame A until just before B (`A-B:Key`), move or click the mouse, turn the wheel, press pad buttons and hold sticks |
| `RYO_HEADLESS=1` | Dev builds: no window; render offscreen one fixed timestep per frame, as fast as possible rather than paced to the clock, with no audio, real gamepads (`RYO_INPUT`'s pad entries drive a virtual one), saved preferences, debug keys (F1, F9, F10, F12) or timing overlays |

Files are named `frame-000060.png` at game resolution (no letterboxing). In dev builds, F12 or the console command `debug:capture [path]` saves the next frame.

`RYO_INPUT` keys use winit `KeyCode` names in any case (`KeyA`, `Digit1`, `ArrowLeft`, `Space`, `Enter`, `Escape`, `Tab`, `F1`), separated by spaces or commas. A tap stays down until a game update has seen it; pressing a key the script still holds waits until it is up, without delaying other entries. Scripted keys send no typed text, so they cannot type into UI text fields. `N:Mouse@XxY` moves the mouse to game coordinates; `Click` and `RightClick` work like keys, optionally at `@XxY`, and a range (`A-B:Click@XxY`) holds the button so `Mouse` entries inside it drag. An invalid entry disables the whole script with one warning.

| Entry | Does |
|-------|------|
| `N:Wheel@DXxDY` | turns the mouse wheel by `DX`, `DY` lines on frame `N` (+y up: `Wheel@0x-3` scrolls down) |
| `N:PadSouth`, `A-B:PadDPadDown` | taps or holds a gamepad button: `Pad` followed by a `GamepadButton` name (`PadSouth`, `PadEast`, `PadStart`, `PadLeftBumper`, `PadDPadUp`, ...) |
| `A-B:LeftStick@XxY`, `A-B:RightStick@XxY` | holds a stick at `X`, `Y` (-1..1, +y up) from frame `A`, centred again before frame `B` |

Pad entries drive the connected pad, or a virtual one ("Virtual pad") when none is, so a menu can be played and captured with the D-pad and A alone:

```bash
# Down twice through a pad menu, open the third item, hold the left stick down, turn the wheel
RYO_HEADLESS=1 RYO_INPUT="60:PadDPadDown 80:PadDPadDown 100:PadSouth 140-200:LeftStick@0x-1 220:Wheel@0x-2" \
  RYO_CAPTURE_FRAMES=90,130,230 RYO_CAPTURE_EXIT=1 ryo2d dev
```

```bash
RYO_CAPTURE_FRAMES=120 RYO_CAPTURE_EXIT=1 ryo2d dev

# Open a demo's second and third pages and capture them, without a window
RYO_HEADLESS=1 RYO_INPUT="60:BracketRight 120:BracketRight" \
  RYO_CAPTURE_FRAMES=90,150 RYO_CAPTURE_EXIT=1 ryo2d dev
```

Without `RYO_CAPTURE_EXIT`, a headless run lasts until the scene quits. An invalid `RYO_CAPTURE_*` or `RYO_INPUT` value (or `RYO_CAPTURE_EXIT=1` with no frames) stops a headless run before it starts, with exit code 2.

In a window, `RYO_INPUT` only advances while the window has focus; prefer `RYO_HEADLESS=1` for scripted runs.
