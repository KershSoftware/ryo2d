<p align="center">
  <img src="src/assets/logo.png" alt="Ryo2D Logo" width="128">
</p>

# Ryo2D

*Tagline:*
A Rust 2D game engine designed with agent-assisted game development in mind from the beginning. Build professional games with AI agents or by hand with a sane API and traditional code -- your choice.

*Reality:*
I started learning Rust and then I decided to start building a 2D game engine to apply the language; all while using agentic tools.

**Should you use this engine today?** Probably not -- it's constantly evolving and is one big experiment. Use at your own risk.

## Example Games

<p align="center">
  <img src="docs/assets/flappy.gif" alt="Flappy" height="200">
  <img src="docs/assets/breakout.gif" alt="Breakout" height="200">
</p>
<p align="center">
  <img src="docs/assets/bullet-hell.gif" alt="Bullet Hell" width="500">
</p>
<p align="center">
  <img src="docs/assets/leap.gif" alt="Leap" width="500">
</p>

## Quick Start

### Install

```powershell
# Windows
git clone https://github.com/Kersheh/ryo2d.git
cd ryo2d
./install # Windows
./install.sh # macOS / Linux
```

### Create a Game

```bash
ryo2d init my-game
cd my-game
ryo2d dev
```

When you run `ryo2d init`, you'll be prompted to install **Agent Skills**—domain-specific knowledge that helps AI assistants understand the engine's APIs and patterns. This is **highly recommended** for agentic game development.

### A First Scene

A game is a stack of scenes over one ECS world. `update` runs at the fixed rate, `ui` once a frame before it:

```rust
use ryo2d::prelude::*;

#[derive(Default)]
struct Level {
  player: Option<Entity>,
  presses: u32,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(32, 32, 80, 200, 255, 255), "player");
    let player = ctx
      .world
      .spawn((Transform::from_xy(400.0, 300.0), Renderable::sprite(sprite)))
      .id();
    self.player = Some(player);
  }

  // Every fixed step (60 per second by default); dt is the step length
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    let mut dx = 0.0;
    if ctx.input.key_down(KeyCode::KeyA) {
      dx -= 1.0;
    }
    if ctx.input.key_down(KeyCode::KeyD) {
      dx += 1.0;
    }
    if ctx.input.key_pressed(KeyCode::Space) {
      self.presses += 1;
    }
    if let Some(player) = self.player
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(player)
    {
      transform.translation.x += dx * 200.0 * dt;
    }
    SceneAction::Continue
  }

  // Once a frame, before the fixed steps; input the UI uses never reaches update
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_text(&format!("Space presses: {}", self.presses), 10.0, 10.0, Color::WHITE);
    if ui.button("Quit", Rect::new(10.0, 30.0, 80.0, 24.0)) {
      return SceneAction::Quit;
    }
    SceneAction::Continue
  }
}

fn main() -> EngineResult<()> {
  EngineBuilder::new()
    .title("My Game")
    .resolution(800, 600)
    .fixed_hz(60)
    .start_scene(Level::default())
    .build()
    .run()
}
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `ryo2d init <name>` | Create a new game project |
| `ryo2d build` | Build production release |
| `ryo2d run` | Run the production build |
| `ryo2d dev` | Development mode with hot-reload |
| `ryo2d export` | Package release into distributable zip |
| `ryo2d install-skills` | Add Agent Skills to existing project |
| `ryo2d netplay` | Test multiplayer with friends over internet |

## Dev Build Performance

New projects include optimized dependencies for playable dev builds. If you're upgrading an existing project or experiencing slow dev builds, add this to your `Cargo.toml`:

```toml
[profile.dev.package."*"]
opt-level = 2
```

This optimizes dependencies (physics, rendering) while keeping your game code fast to compile. `ryo2d dev` will warn you if this is missing.

## Core Features

### Architecture
- **ECS** — Ryo2D's ECS (built on `bevy_ecs`): queries, systems in named phases, resources, change detection
- **Frame Loop** — Fixed-step gameplay with real, game and fixed clocks, render interpolation, UI before gameplay
- **Transforms** — Local `Transform` and engine-written `GlobalTransform` with parent/child hierarchy
- **Scene Management** — Stack-based scenes with transitions and scene-owned systems
- **Messages** — Typed messages between systems and scenes
- **Object Pooling** — Pre-allocated entity pools for bullets, particles, effects
- **State Machines** — Generic FSM and state stack for player states, AI, menus

### Rendering
- **Sprites** — One GPU batch for sprites, particles, trails and lines, with z-ordering, per-sprite shaders, blend modes (alpha, additive, multiply, screen) and mipmapped textures
- **Animation** — Sprite sheets and state machine controllers
- **Skeletal Animation** — Bone hierarchies, per-bone keyframe tracks, crossfade blending, FSM controllers
- **Tweening** — Transform tweens, generic property tweens, sequences, 25 easing functions
- **Parallax** — Layered scrolling backgrounds with infinite scroll, laid out for every camera
- **2D Lighting** — Point lights, spotlights, volumetric effects, atmospheric scattering
- **Shadow System** — Hard shadows, soft shadows, drop/blob/contact shadows, ambient occlusion
- **Trails and Lines** — Motion trails and shapes with joins, caps, fade and taper effects
- **Debug Drawing** — `ctx.debug_draw()` lines, rectangles, circles, arrows and crosses, one pixel wide at any zoom
- **Particles** — GPU-batched particle system with emitters, attractors, and presets
- **Post-Processing** — Shader pipeline (bloom, CRT, vignette, chromatic aberration, etc.)
- **Render Pipeline** — Pass-based, per camera then per frame; games toggle built-in passes or add their own
- **Resolution Model** — Resolution-independent (sharp at any window size and on HiDPI) or pixel art (a fixed target scaled by whole numbers)
- **Web** — The engine builds for wasm32 and runs in browsers with WebGPU

### Physics & Collision
- **Rapier2D** — Full physics simulation with rigid bodies
- **Collision Shapes** — Box and circle colliders with trigger support
- **Collision Messages** — Started/stopped messages for gameplay logic

### Cameras
- **Cameras as Entities** — Follow targets, zoom, rotation, bounds clamping, screen/world conversion, smooth interpolation
- **Split Screen & Minimaps** — Any number of cameras with viewports, render order and render layers
- **Render to Texture** — Cameras render into textures that sprites show
- **Screen Shake** — Trauma-based shake with smooth noise

### Input
- **Keyboard & Mouse** — Key states, mouse position, scroll
- **Gamepad** — Controller support via gilrs (buttons, sticks, triggers)

### Game Feel
- **Time Scale** — Slow-motion and fast-forward effects
- **Hitstop** — Freeze frames for impact feedback
- **Juice** — Knockback, squash and stretch, camera shake

### Audio
- **Kira Backend** — Music and sound effects with volume control

### UI
- **Builders in `Scene::ui`** — Widgets drawn each frame from the game's own values, with absolute drawing for HUDs
- **Widgets** — Buttons, text input, dropdowns, sliders, checkboxes, toggles, tabs, scroll views, slots, prompts and rebinding rows
- **Layout** — Columns, rows, grids, stacks and panels laid out as flexbox and grid
- **Gamepad navigation** — Focus scopes, modals and popups played with the D-pad, A and B alone
- **Theming** — Palettes, looks per widget state, variants, nine-slice skins and sounds, from RON files that hot-reload in debug builds

### Gameplay Systems
- **Pathfinding** — A* with nav agents and grid-based navigation
- **Procedural Generation** — BSP dungeons, cellular automata, WFC, noise (optional `ryo2d-procgen` crate)
- **Inventory System** — Item registry, slot-based containers, equipment, loot tables (optional `ryo2d-inventory` crate)

### Networking
- **Client-Server** — Dedicated and listen server modes
- **Transport Layer** — UDP (LAN) or WebRTC (internet with NAT traversal)
- **Replication** — Automatic entity/component synchronization
- **RPCs** — Remote procedure calls with targeting
- **Netplay Testing** — `ryo2d netplay` for quick multiplayer testing

### Persistence
- **Save System** — Player settings plus compressed game save slots
- **Configuration** — TOML-based project settings, JSON user preferences

### Development Tools
- **Hot Reload** — Asset changes apply without restart
- **Debug Visualization** — Colliders, velocities, origins, FPS overlay
- **Developer Console** — Built-in and game commands
- **Headless Runs** — Scripted input and frame capture without a window, for tests and agents
- **Profiler Window** — Real-time flamegraph with CPU/GPU timing (requires `--features profiler`)
- **Logger Window** — Live log viewer with level/context filtering

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| [bevy_ecs](https://crates.io/crates/bevy_ecs) | Entity-Component-System |
| [rapier2d](https://crates.io/crates/rapier2d) | 2D Physics |
| [wgpu](https://crates.io/crates/wgpu) | GPU Rendering |
| [winit](https://crates.io/crates/winit) | Windowing |
| [kira](https://crates.io/crates/kira) | Audio |
| [image](https://crates.io/crates/image) | Image loading |
| [puffin](https://crates.io/crates/puffin) | Profiler instrumentation |

## Documentation

- **API reference:** rustdoc, `cargo doc --open`
- **Using the engine:** the agent skills in `.claude/skills/` (`ryo-api` is the full API reference), shipped with `ryo2d` and kept current with the code
- **Design notes:** a nested `AGENTS.md` in each module, and the specs in `docs/specs/`
- **Big picture:** `docs/architecture/` (engine overview, network architecture)

## License

MIT
