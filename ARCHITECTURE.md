# Ryo2D Architecture

## High-Level Overview

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                                 main.rs                                      │
│                            EngineBuilder::new()                              │
│                               .title()                                       │
│                               .resolution()                                  │
│                               .with_game(MyGame)                             │
│                               .build()                                       │
│                               .run()                                         │
└──────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                                  Engine                                      │
│                                                                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌──────────────────────┐ │
│  │EngineConfig │  │  TimeState  │  │ InputState  │  │    World (hecs)      │ │
│  │             │  │             │  │             │  │                      │ │
│  │ • title     │  │ • delta_time│  │ • keys_down │  │ • Archetype storage  │ │
│  │ • width     │  │ • fixed_dt  │  │ • mouse_pos │  │ • Generational IDs   │ │
│  │ • height    │  │ • accumulat.│  │ • scroll    │  │ • Type-safe queries  │ │
│  │ • fps       │  │ • frame_cnt │  │             │  │                      │ │
│  │ • physics_hz│  │             │  │             │  │                      │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └──────────────────────┘ │
│                                                                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌──────────────────────┐ │
│  │   Camera    │  │AssetManager │  │AudioManager │  │ RapierPhysicsWorld   │ │
│  │             │  │             │  │             │  │                      │ │
│  │ • position  │  │ • sprites   │  │ • sfx       │  │ • RigidBodySet       │ │
│  │ • zoom      │  │ • hot reload│  │ • music     │  │ • ColliderSet        │ │
│  │ • viewport  │  │ • async load│  │ • volume    │  │ • PhysicsPipeline    │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └──────────────────────┘ │
│                                                                              │
│  ┌───────────────────────────────────────────────────────────────────────┐   │
│  │                     Window (winit) + WgpuRenderer                     │   │
│  └───────────────────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Game Loop (Fixed Timestep)

```
┌───────────────────────────────────────────────────────────────────────────┐
│                         Frame (RedrawRequested)                           │
├───────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│   1. time.begin_frame()            Calculate delta, add to accumulator    │
│              │                                                            │
│              ▼                                                            │
│   ┌──────────────────────┐                                                │
│   │  while accumulator   │◄────────────────────────────┐                  │
│   │  >= fixed_timestep   │                             │                  │
│   └──────────┬───────────┘                             │                  │
│              │ yes                                     │                  │
│              ▼                                         │                  │
│   2. update(fixed_dt)           Defaults 60Hz updates  │                  │
│      • game.update()            (deterministic)        │                  │
│      • Rapier physics step                             │                  │
│      • Sync positions                                  │                  │
│              │                                         │                  │
│              ▼                                         │                  │
│      accumulator -= fixed_dt ──────────────────────────┘                  │
│              │                                                            │
│              ▼ (exit loop)                                                │
│   3. input.begin_frame()           Clear per-frame input (only if        │
│              │                     update ran, to preserve input)        │
│              ▼                                                            │
│   4. render(alpha)                 Variable rate rendering                │
│      • Z-sorted sprites            (with interpolation)                   │
│      • Frustum culling                                                    │
│              │                                                            │
│              ▼                                                            │
│   5. renderer.render()             Upload pixel buffer to GPU texture,    │
│                                    draw fullscreen quad with scaling      │
│                                                                           │
└───────────────────────────────────────────────────────────────────────────┘
```

## ECS Architecture (hecs)

Ryo2D uses [hecs](https://github.com/Ralith/hecs), a high-performance archetype-based ECS.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                            World (hecs::World)                            │
│                                                                           │
│  Archetype Storage (entities with same component types stored together)   │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  Archetype A: (Transform, Velocity, RigidBody, Collider, Sprite)   │   │
│  │  ┌─────────┬─────────┬─────────┬─────────┬─────────┐               │   │
│  │  │ Entity0 │ Entity1 │ Entity2 │ ...     │ EntityN │  ← Cache-     │   │
│  │  │ T,V,R,C │ T,V,R,C │ T,V,R,C │         │ T,V,R,C │    friendly   │   │
│  │  └─────────┴─────────┴─────────┴─────────┴─────────┘               │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  Archetype B: (Transform, SpriteRenderer)  ← Background tiles      │   │
│  │  ┌─────────┬─────────┬─────────┐                                   │   │
│  │  │ Entity  │ Entity  │ Entity  │                                   │   │
│  │  │ T, S    │ T, S    │ T, S    │                                   │   │
│  │  └─────────┴─────────┴─────────┘                                   │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Generational IDs: Entity = (index: u32, generation: u32)                 │
│  Stale handles safely return None instead of accessing wrong entity       │
└───────────────────────────────────────────────────────────────────────────┘

Usage:
  // Spawn with bundle
  let entity = world.spawn((Transform::new(x, y), Velocity::zero(), RigidBody::new()));
  
  // Query
  for (entity, (transform, velocity)) in world.query::<(&Transform, &Velocity)>() { ... }
  
  // Single component access
  let pos = world.get_component::<Transform>(entity)?;
```

## Physics System (Rapier2D)

Physics is handled by [Rapier2D](https://rapier.rs/), a professional-grade physics engine.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                          RapierPhysicsWorld                               │
│                                                                           │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐    │
│  │  RigidBodySet   │  │   ColliderSet   │  │   PhysicsPipeline       │    │
│  │                 │  │                 │  │                         │    │
│  │ Dynamic bodies  │  │ Box, Circle     │  │ Broad phase (sweep)     │    │
│  │ Kinematic       │  │ Sensors/Trigger │  │ Narrow phase (shapes)   │    │
│  │ Static          │  │ Restitution     │  │ Constraint solving      │    │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────┘    │
│                                                                           │
│  Entity ↔ Handle Mapping                                                  │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  entity_to_body: HashMap<Entity, RigidBodyHandle>                  │   │
│  │  collider_to_entity: HashMap<ColliderHandle, Entity>               │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Collision Events (via ChannelEventCollector)                             │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  PhysicsCollisionEvent { entity_a, entity_b, Started/Stopped }     │   │
│  └────────────────────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────────────────────┘

Physics Loop:
  1. Auto-register new entities (RigidBody + Collider + Transform)
  2. Sync kinematic positions from ECS → Rapier
  3. physics.step(gravity) — Rapier simulates
  4. Sync dynamic positions from Rapier → ECS
  5. Collision events available via ctx.collision_events
```

## Render Pipeline

```
┌───────────────────────────────────────────────────────────────────────────┐
│                             render                                 │
├───────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│   1. Get frame buffer from WgpuRenderer                                   │
│      frame: &mut [u8] (RGBA, 4 bytes per pixel)                           │
│                                                                           │
│   2. Clear background (dark blue)                                         │
│                                                                           │
│   3. Rebuild render cache if dirty                                        │
│      ┌──────────────────────────────────────────────────────────────┐     │
│      │  render_cache: Vec<(Entity, z_order)>  — sorted by z_order   │     │
│      │  Only rebuilt when entities spawn/despawn                    │     │
│      └──────────────────────────────────────────────────────────────┘     │
│                                                                           │
│   4. For each entity in z-order:                                          │
│      • Get Transform + SpriteRenderer                                     │
│      • Camera.world_to_screen() — apply camera offset + zoom              │
│      • Frustum culling — skip if not visible                              │
│      • sprite.draw() — alpha blending, screen clipping                    │
│                                                                           │
│   5. renderer.render() → Upload pixel buffer to GPU texture               │
│      • Upload CPU pixel buffer to GPU texture                             │
│      • Draw fullscreen quad with WGSL shader                              │
│      • Shader applies viewport scaling for letterbox/pillarbox            │
└───────────────────────────────────────────────────────────────────────────┘
```

## Input Flow

```
┌──────────────────┐      ┌──────────────────┐      ┌──────────────────┐
│   winit Events   │─────►│   InputState     │─────►│  Game Systems    │
└──────────────────┘      └──────────────────┘      └──────────────────┘

WindowEvent::KeyboardInput ───► handle_keyboard()     ───► keys_down/pressed
WindowEvent::MouseInput    ───► handle_mouse_button() ───► buttons_down
WindowEvent::CursorMoved   ───► handle_mouse_move()   ───► mouse_x/y
WindowEvent::MouseWheel    ───► handle_scroll()       ───► scroll_delta

                     ┌─────────────────────────────────────────────┐
                     │             Per-Frame State                 │
                     │                                             │
Events accumulate    │  keys_pressed     (cleared each frame)      │
between frames  ────►│  keys_released    (cleared each frame)      │
                     │  mouse_delta      (cleared each frame)      │
                     │  scroll_delta     (cleared each frame)      │
                     │                                             │
Persistent state     │  keys_down        (tracks held keys)        │
                     │  mouse_position   (current position)        │
                     └─────────────────────────────────────────────┘
```

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `hecs` | High-performance archetype ECS with generational IDs |
| `rapier2d` | Physics simulation, collision detection/response |
| `winit` | Cross-platform windowing and input events |
| `wgpu` | GPU abstraction for rendering |
| `bytemuck` | Safe transmutation for shader uniforms |
| `pollster` | Minimal async runtime for wgpu initialization |
| `kira` | Game audio (sound effects, music, streaming) |
| `image` | Image loading (PNG, JPEG) |
| `notify` | Filesystem watching for hot reload |
| `thiserror` | Ergonomic error types |
| `glam` | Math types (Vec2, Vec3, Mat4) |
| `crossbeam-channel` | Multi-producer channels for Rapier events |
| `ron` | Rusty Object Notation for save data serialization |
| `flate2` | Compression for save files |
| `toml` | TOML parsing for configuration (Ryo.toml, Cargo.toml) |
| `serde` | Serialization framework |

## Coordinate Systems

```
World Space (pixels)                       Screen Space
(origin at world 0,0)                      (0,0 at top-left)

          ▲ -Y                                   0 ─────────────► X
          │                                      │
          │      ●───────────●                   │   (after camera transform)
          │      │  Player   │                   │
◄─────────┼──────┤ (400,300) │                   │       ●───────────●
    -X    │      ●───────────●                   │       │  Player   │
          │            ▲                         │       │  (center) │
          │            │                         │       ●───────────●
          │       Camera follows                 │
          ▼ +Y                                   ▼ Y

Rapier Space (meters)
  PIXELS_PER_METER = 100.0
  100 world pixels = 1 Rapier meter
  
  Transform.x = 500 pixels → Rapier x = 5.0 meters
```

## Data Flow

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   Spawn      │────►│   Physics    │────►│    Sync      │────►│   Render     │
│              │     │   Register   │     │              │     │              │
│ world.spawn( │     │              │     │  Rapier →    │     │  Z-sorted    │
│   Transform, │     │ Add to       │     │  Transform   │     │  sprites     │
│   RigidBody, │     │ RigidBodySet │     │  + Velocity  │     │              │
│   Collider   │     │ ColliderSet  │     │              │     │              │
│ )            │     │              │     │              │     │              │
└──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘
                            │
                            ▼
                     ┌──────────────┐
                     │  Collision   │
                     │   Events     │
                     │              │
                     │ PhysicsCol-  │
                     │ lisionEvent  │
                     │ Started/     │
                     │ Stopped      │
                     └──────────────┘
```

## Scene Management

```
┌───────────────────────────────────────────────────────────────────────────┐
│                          SceneManager                                     │
│                                                                           │
│  Scene Stack (Vec<Box<dyn Scene>>)                                        │
│  ┌──────────────────────────────────────────────────────────────────┐     │
│  │  [MenuScene]           ← Top (active, receives update/render)    │     │
│  │  [GameScene]           ← Paused (not updated)                    │     │
│  └──────────────────────────────────────────────────────────────────┘     │
│                                                                           │
│  Actions (processed after update):                                        │
│  • Push(scene)    — Add new scene on top                                  │
│  • Pop            — Remove current scene, resume previous                 │
│  • Replace(scene) — Replace current scene with new one                    │
│  • Clear          — Remove all scenes                                     │
│                                                                           │
│  Scene Lifecycle:                                                         │
│  1. on_enter()    — Called when scene becomes active                      │
│  2. update()      — Called every frame while active                       │
│  3. render()      — Called every frame while active                       │
│  4. on_exit()     — Called when scene is removed                          │
└───────────────────────────────────────────────────────────────────────────┘

Usage:
  scene_manager.push(Box::new(MenuScene::new()));
  scene_manager.replace(Box::new(GameScene::new(level)));
```

## Save System

```
┌───────────────────────────────────────────────────────────────────────────┐
│                           SaveManager                                     │
│                                                                           │
│  Save Directory: workspace.save_data_dir()                                │
│  ┌──────────────────────────────────────────────────────────────────┐     │
│  │  data/                                                           │     │
│  │  ├── slot1.save     (compressed RON)                             │     │
│  │  ├── slot2.save                                                  │     │
│  │  └── system.save    (game settings, progress)                    │     │
│  └──────────────────────────────────────────────────────────────────┘     │
│                                                                           │
│  Features:                                                                │
│  • Named slots (e.g., "autosave", "slot1")                                │
│  • Automatic compression (flate2/gzip)                                    │
│  • Metadata (timestamp, version, description)                             │
│  • RON serialization (human-readable, versionable)                        │
│  • List/delete saves                                                      │
│                                                                           │
│  Types:                                                                   │
│  • Game saves    — Full world state, entities, components                 │
│  • System saves  — Settings, keybinds, preferences                        │
│  • Raw saves     — Custom binary data                                     │
└───────────────────────────────────────────────────────────────────────────┘

Usage:
  let save_mgr = workspace.create_save_manager()?;
  save_mgr.save_game("slot1", &game_data)?;
  let loaded = save_mgr.load_game::<GameData>("slot1")?;
```

## Workspace System

```
┌───────────────────────────────────────────────────────────────────────────┐
│                            Workspace                                      │
│                                                                           │
│  Project Structure:                                                       │
│  ┌──────────────────────────────────────────────────────────────────┐     │
│  │  my_game/                                                        │     │
│  │  ├── Ryo.toml         (case-insensitive: ryo.toml also works)    │     │
│  │  ├── Cargo.toml                                                  │     │
│  │  ├── main.rs                                                     │     │
│  │  ├── assets/          (sprites, sounds, etc.)                    │     │
│  │  └── data/            (save files)                               │     │
│  └──────────────────────────────────────────────────────────────────┘     │
│                                                                           │
│  Ryo.toml:                                                                │
│  [project]                                                                │
│  name = "My Game"                                                         │
│                                                                           │
│  [paths]                                                                  │
│  entry_point = "main.rs"   # or "src/main.rs"                             │
│  assets = "assets"                                                        │
│  save_data = "data"                                                       │
│                                                                           │
│  Discovery:                                                               │
│  • Workspace::discover() searches cwd and 3 parent directories            │
│  • Used by CLI commands to find project root                              │
│  • Provides canonical paths for assets, saves, etc.                       │
└───────────────────────────────────────────────────────────────────────────┘

Usage:
  let workspace = Workspace::discover()?;
  let assets_dir = workspace.assets_dir();
  let save_manager = workspace.create_save_manager()?;
```

## CLI System

```
┌───────────────────────────────────────────────────────────────────────────┐
│                        ryo2d CLI Commands                                 │
│                                                                           │
│  ryo2d build                                                              │
│  ├─► cargo build --release                                                │
│  └─► Output: target/release/my_game                                       │
│                                                                           │
│  ryo2d run                                                                │
│  ├─► Check: target/release/my_game exists?                                │
│  ├─► Read Cargo.toml for binary name                                      │
│  └─► Execute: ./target/release/my_game                                    │
│                                                                           │
│  ryo2d dev                                                                │
│  ├─► cargo run (debug build)                                              │
│  └─► (Future: file watching + hot reload)                                 │
│                                                                           │
│  Workflow Integration:                                                    │
│  ├─► Workspace-aware (discovers Ryo.toml)                                 │
│  ├─► Runs from any subdirectory                                           │
│  └─► Uses Cargo for compilation                                           │
└───────────────────────────────────────────────────────────────────────────┘

Installation:
  ./install.ps1  (Windows) or ./install.sh (Unix)
  Copies/symlinks to ~/.ryo2d/bin/
  Optionally adds to PATH
```

## UI System

```
┌───────────────────────────────────────────────────────────────────────────┐
│                         Immediate-Mode UI                                 │
│                                                                           │
│  Design Philosophy:                                                       │
│  • Call draw functions each frame                                         │
│  • No persistent widget state                                             │
│  • Simple and predictable                                                 │
│                                                                           │
│  Components:                                                              │
│  ┌──────────────────────────────────────────────────────────────────┐     │
│  │  BitmapFont                                                      │     │
│  │  ├── Embedded debug font (fixed-width)                           │     │
│  │  ├── Character lookup and positioning                            │     │
│  │  └── Text rendering with color                                   │     │
│  └──────────────────────────────────────────────────────────────────┘     │
│                                                                           │
│  ┌──────────────────────────────────────────────────────────────────┐     │
│  │  UiContext (Drawing API)                                         │     │
│  │  ├── draw_text(text, x, y, color)                                │     │
│  │  ├── draw_rect(rect, color)                                      │     │
│  │  ├── draw_bordered_rect(rect, fill, border)                      │     │
│  │  ├── draw_progress_bar(rect, value, fill, bg)                    │     │
│  │  └── button(label, rect) -> bool                                 │     │
│  └──────────────────────────────────────────────────────────────────┘     │
│                                                                           │
│  Types:                                                                   │
│  • Color   — RGBA with named constants (RED, GREEN, BLUE, etc.)           │
│  • Rect    — Screen-space rectangle (x, y, width, height)                 │
│  • Anchor  — Positioning helper (TopLeft, Center, BottomRight, etc.)      │
│                                                                           │
│  Use Cases:                                                               │
│  • HUD elements (health bars, score display)                              │
│  • Debug overlays (FPS counter, system info)                              │
│  • Simple menus and buttons                                               │
│  • Text rendering for gameplay                                            │
└───────────────────────────────────────────────────────────────────────────┘

Usage:
  fn render_ui(&mut self, ui: &mut UiContext) {
      ui.draw_text(&format!("Score: {}", score), 10.0, 10.0, Color::WHITE);
      ui.draw_progress_bar(health_bar_rect, health / 100.0, Color::RED, Color::GRAY);
      if ui.button("Pause", pause_button_rect) {
          self.paused = true;
      }
  }
```
