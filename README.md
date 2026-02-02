# Ryo2D

*Tagline:*
A Rust 2D game engine designed with agent-assisted development in mind from the beginning. Build professional games with AI agents or by hand with traditional code -- your choice.

*Reality:*
I started learning Rust and then I decided to start building a 2D game engine to apply the language; all while using agentic tools.

## Quick Start

### Install

```powershell
# Windows
git clone https://github.com/Kersheh/ryo2d.git
cd ryo2d
./install.ps1
```

```bash
# macOS / Linux
git clone https://github.com/Kersheh/ryo2d.git
cd ryo2d
./install.sh
```

### Create a Game

```bash
ryo2d init my-game
cd my-game
cargo run
```

When you run `ryo2d init`, you'll be prompted to install **Agent Skills**—domain-specific knowledge that helps AI assistants understand the engine's APIs and patterns. This is **highly recommended** for agentic game development.

## CLI Commands

| Command | Description |
|---------|-------------|
| `ryo2d init <name>` | Create a new game project |
| `ryo2d build` | Build production release |
| `ryo2d run` | Run the production build |
| `ryo2d dev` | Development mode with hot-reload |
| `ryo2d install-skills` | Add Agent Skills to existing project |

## Core Features

- **ECS Architecture** — Entity-Component-System for clean game logic
- **Physics** — Rapier2D integration with collision events
- **Sprite Rendering** — GPU-batched sprites with z-ordering
- **Animation** — Sprite sheets and state machine controllers
- **Audio** — Music and sound effects with Kira
- **Scene Management** — Stack-based scenes with transitions
- **UI System** — Hybrid immediate/retained mode with theme support, layout containers, text inputs, dropdowns, scroll views, and focus management
- **Post-Processing** — Shader pipeline for visual effects
- **Hot Reload** — Asset changes apply without restart
- **Configuration** — TOML-based project and user settings

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| [hecs](https://crates.io/crates/hecs) | Entity-Component-System |
| [rapier2d](https://crates.io/crates/rapier2d) | 2D Physics |
| [wgpu](https://crates.io/crates/wgpu) | GPU Rendering |
| [winit](https://crates.io/crates/winit) | Windowing |
| [kira](https://crates.io/crates/kira) | Audio |
| [image](https://crates.io/crates/image) | Image loading |

## Documentation

Build and view the documentation locally:

```bash
# Build HTML docs (mdbook guide + rustdoc API)
./build-docs.ps1   # Windows
./build-docs.sh    # macOS / Linux

# Open in browser
start docs/book/index.html        # Windows
open docs/book/index.html         # macOS
xdg-open docs/book/index.html     # Linux
```

## License

MIT
