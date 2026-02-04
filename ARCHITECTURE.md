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
│   3. input.begin_frame()           Clear per-frame input (only if         │
│              │                     update ran, to preserve input)         │
│              ▼                                                            │
│   4. render(alpha)                 Variable rate rendering                │
│      • game.render() callback      (with interpolation)                   │
│      • game.render_gpu() callback                                         │
│      • game.render_ui() callback                                          │
│              │                                                            │
│              ▼                                                            │
│   5. pipeline.execute()            Pass-based render pipeline             │
│      • Clear → Parallax → CpuSprite → Upload → GpuSprite                  │
│      • Trail → Lighting → UI → Effects → Present                          │
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

Ryo2D uses a **pass-based render pipeline** where each rendering phase is a separate `RenderPass`. Passes declare dependencies and execute in topological order.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                           RenderPipeline                                  │
│                                                                           │
│  Pass-Based Architecture:                                                 │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │                                                                    │   │
│  │   ClearPass ──► ParallaxPass ──► CpuSpritePass ──► UploadPass      │   │
│  │                                                          │         │   │
│  │                                                          ▼         │   │
│  │   PresentPass ◄── EffectsPass ◄── UiPass ◄── LightingPass          │   │
│  │        │              │             │             │                │   │
│  │        │              │             │             ▼                │   │
│  │        │              │             │        TrailPass             │   │
│  │        │              │             │             │                │   │
│  │        │              │             │             ▼                │   │
│  │        │              │             │        GpuSpritePass         │   │
│  │        ▼              ▼             ▼             ▼                │   │
│  │    [Screen]     [Scratch A/B]  [Game Tex]   [Game Tex]             │   │
│  │                  (ping-pong)                                       │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Built-in Passes:                                                         │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  clear       │  Clear pixel buffer to background color             │   │
│  │  parallax    │  GPU-rendered parallax background layers            │   │
│  │  cpu_sprite  │  Software sprite rendering to pixel buffer          │   │
│  │  upload      │  Upload CPU pixel buffer to GPU texture             │   │
│  │  gpu_sprite  │  Batched GPU sprite rendering                       │   │
│  │  trail       │  Motion trail rendering                             │   │
│  │  lighting    │  2D lighting with shadows                           │   │
│  │  ui          │  User interface overlay                             │   │
│  │  effects     │  Post-processing (CRT, vignette, etc.)              │   │
│  │  present     │  Final output to swapchain                          │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Dependency Resolution:                                                   │
│  • Passes declare dependencies via dependencies() -> &[&str]              │
│  • Pipeline computes execution order via topological sort                 │
│  • Circular dependencies panic at pipeline creation                       │
│  • Missing dependencies are silently ignored (optional passes)            │
│                                                                           │
│  RenderContext (shared state):                                            │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  • device, queue, encoder    — wgpu resources                      │   │
│  │  • game_texture              — main render target                  │   │
│  │  • scratch_a, scratch_b      — ping-pong buffers for effects       │   │
│  │  • pixel_buffer              — CPU buffer for software rendering   │   │
│  │  • camera                    — world-to-screen transforms          │   │
│  │  • sprite_gpu, batch_collector — GPU sprite system                 │   │
│  │  • trail_renderer, lighting  — specialized renderers               │   │
│  │  • current_source            — which texture has latest result     │   │
│  └────────────────────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────────────────────┘

Pipeline Configuration:
  // Disable a pass at runtime
  pipeline.set_enabled("lighting", false);

  // Add custom pass (inserted by dependency order)
  pipeline.add_pass(MyCustomPass::new());

  // Remove a pass
  pipeline.remove_pass("trail");

  // Query pass state
  if pipeline.is_enabled("effects") { ... }

Custom Pass Example:
  struct MyPass { enabled: bool }

  impl RenderPass for MyPass {
      fn id(&self) -> &'static str { "my_pass" }

      fn dependencies(&self) -> &[&'static str] {
          &["gpu_sprite"]  // Run after GPU sprites
      }

      fn execute(&mut self, ctx: &mut RenderContext) {
          // Access ctx.encoder, ctx.device, ctx.pixel_buffer, etc.
      }

      fn should_execute(&self, ctx: &RenderContext) -> bool {
          true  // Return false to skip this frame
      }
  }
```

## Post-Processing Effects

Post-processing is handled by the `EffectsPass` in the render pipeline, using ping-pong buffers for effect chaining.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                         EffectsPass (part of pipeline)                    │
│                                                                           │
│  Ping-Pong Buffer Flow:                                                   │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │                                                                    │   │
│  │   Game        Effect 1      Effect 2      Effect N       Present   │   │
│  │  Texture  ──►  (Fade)   ──►  (CRT)    ──►   ...     ──►   Pass     │   │
│  │     │           │             │             │             │        │   │
│  │     ▼           ▼             ▼             ▼             ▼        │   │
│  │  [Scratch A] [Scratch B]  [Scratch A]   [Scratch B]   [Screen]     │   │
│  │                                                                    │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Available Effects:                                                       │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  Fade                │  Screen fade to color (scene transitions)   │   │
│  │  ColorTint           │  Color grading/mood (sepia, night vision)   │   │
│  │  Vignette            │  Darkened edges (cinematic look)            │   │
│  │  ScreenFlash         │  One-shot flash with auto-decay (damage)    │   │
│  │  CRT                 │  Scanlines, curvature, RGB offset (retro)   │   │
│  │  ChromaticAberration │  RGB color fringing (glitch/lens effect)    │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Key Types:                                                               │
│  • EffectsManager     — Manages active effects, timing, convenience API   │
│  • PostProcessEffect  — Enum of effect variants with parameters           │
│  • EffectParams       — 64-byte uniform struct for shader parameters      │
└───────────────────────────────────────────────────────────────────────────┘

Usage:
  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
      // Persistent effects
      ctx.effects.set_vignette(0.3);
      ctx.effects.set_crt(0.2, 0.01, 0.001);

      // Timed effects (auto-decay)
      if player_hit {
          ctx.effects.flash(Color::RED, Duration::from_millis(150));
      }

      // Scene transitions
      ctx.effects.set_fade(self.fade_progress);
  }
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
│              │     │   Register   │     │              │     │   Pipeline   │
│ world.spawn( │     │              │     │  Rapier →    │     │              │
│   Transform, │     │ Add to       │     │  Transform   │     │ Pass-based   │
│   RigidBody, │     │ RigidBodySet │     │  + Velocity  │     │ execution    │
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

Ryo2D uses a **hybrid UI system** combining immediate-mode and retained-mode approaches.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                          Hybrid UI System                                 │
│                                                                           │
│  Immediate Mode (stateless):                                              │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  • Call draw functions each frame                                  │   │
│  │  • No persistent widget state                                      │   │
│  │  • Buttons return true when clicked                                │   │
│  │  • Ideal for: HUDs, debug overlays, simple menus                   │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Retained Mode (stateful):                                                │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  • Caller owns state structs (TextInputState, DropdownState, etc.) │   │
│  │  • State persists across frames (cursor, selection, scroll)        │   │
│  │  • Ideal for: forms, text input, complex menus                     │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Immediate Widgets:                                                       │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  button(label, rect)           │  Returns true when clicked        │   │
│  │  label(text, rect)             │  Static text display              │   │
│  │  progress_bar(rect, value)     │  0.0-1.0 fill bar                 │   │
│  │  slider(rect, value, range)    │  Draggable value selector         │   │
│  │  checkbox(rect, checked)       │  Toggle boolean                   │   │
│  │  draw_rect, draw_text, ...     │  Primitives                       │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Retained Widgets:                                                        │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  TextInputState    │  Text field with cursor, selection            │   │
│  │  DropdownState     │  Expandable option list                       │   │
│  │  ScrollViewState   │  Scrollable content region                    │   │
│  │  FocusTrap         │  Keyboard navigation container                │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Layout Containers:                                                       │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  HBox     │  Horizontal layout with spacing                        │   │
│  │  VBox     │  Vertical layout with spacing                          │   │
│  │  Stack    │  Overlapping children (z-order)                        │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Theme System:                                                            │
│  • Centralized styling (colors, spacing, borders)                         │
│  • Per-widget styles (ButtonStyle, TextInputStyle, etc.)                  │
│  • Hot-reload from RON files                                              │
│  • Runtime theme switching                                                │
└───────────────────────────────────────────────────────────────────────────┘

Usage:
  // In scene struct - retained state
  struct MyScene {
      name_input: TextInputState,
      difficulty: DropdownState,
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
      // Immediate mode - no state needed
      ui.draw_text(&format!("Score: {}", score), 10.0, 10.0, Color::WHITE);
      ui.draw_progress_bar(health_rect, health / 100.0, Color::RED, Color::GRAY);

      if ui.button("Start", button_rect) {
          self.start_game();
      }

      // Retained mode - state persists
      ui.text_input(&mut self.name_input, input_rect);
      ui.dropdown(&mut self.difficulty, dropdown_rect, &["Easy", "Normal", "Hard"]);
      ui.finish_dropdowns();  // Call at end for proper z-order
  }
```

## Profiler System

Ryo2D includes a built-in profiler using [puffin](https://github.com/EmbarkStudios/puffin) for hierarchical frame profiling.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                          Profiler Architecture                            │
│                                                                           │
│  puffin (scope collection)                                                │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  profile_scope!("cpu:engine_update")  ← Engine scopes with prefix  │   │
│  │  profile_scope!("gpu:sprite_batching")                             │   │
│  │  profile_scope!("player_update")      ← Game scopes (no prefix)    │   │
│  │                                                                    │   │
│  │  GlobalProfiler collects timing data per-frame                     │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                          │                                                │
│                          ▼                                                │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  FrameView (sink)      │  ProfilerWindow subscribes via sink       │   │
│  │  └► ScopeEntry[]       │  Receives scope data each frame           │   │
│  │     • name             │                                           │   │
│  │     • duration_ns      │                                           │   │
│  │     • depth            │                                           │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                          │                                                │
│                          ▼                                                │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  ProfilerWindow (separate window)                                  │   │
│  │  ┌──────────────────────────────────────────────────────────────┐  │   │
│  │  │  Game Section         ← player_update: 2.1ms                 │  │   │
│  │  │                         enemy_ai: 1.3ms                      │  │   │
│  │  │                         physics_sim: 0.8ms                   │  │   │
│  │  ├──────────────────────────────────────────────────────────────┤  │   │
│  │  │  Engine - CPU Section ← engine_update: 0.5ms                 │  │   │
│  │  │                         animation_system: 0.2ms              │  │   │
│  │  ├──────────────────────────────────────────────────────────────┤  │   │
│  │  │  Engine - GPU Section ← sprite_batching: 1.1ms               │  │   │
│  │  │                         lighting_pass: 0.7ms                 │  │   │
│  │  └──────────────────────────────────────────────────────────────┘  │   │
│  │                                                                    │   │
│  │  • CPU pixel buffer rendering (independent of game renderer)       │   │
│  │  • Mouse hover for scope details                                   │   │
│  │  • Real-time flamegraph visualization                              │   │
│  └────────────────────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────────────────────┘

Scope Categorization:
  ┌─────────────────────────────────────────────────────────────────────┐
  │  Prefix      │  Category      │  Used By                           │
  ├─────────────────────────────────────────────────────────────────────┤
  │  cpu:        │  Engine - CPU  │  Engine internals (physics, ECS)   │
  │  gpu:        │  Engine - GPU  │  Rendering, batching, shaders      │
  │  (none)      │  Game          │  Game developer code               │
  └─────────────────────────────────────────────────────────────────────┘

  Game developers use unprefixed scopes - categorization is automatic:
    profile_scope!("enemy_ai");      // Shows in "Game" section
    profile_scope!("spawn_bullets"); // Shows in "Game" section

  Engine uses prefixed scopes for organization:
    profile_scope!("cpu:physics_step");   // Shows in "Engine - CPU"
    profile_scope!("gpu:submit_batches"); // Shows in "Engine - GPU"

Enabling Profiling:
  // In Ryo.toml
  [debug]
  puffin_profiler = true

  // Or via CLI
  ryo2d dev --profiler

  // Compile with feature
  cargo run --features profiler

Usage in Game Code:
  use ryo2d::prelude::*;

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
      profile_scope!("game_update");  // Automatic "Game" category

      {
          profile_scope!("player_movement");
          self.update_player(ctx, dt);
      }

      {
          profile_scope!("enemy_ai");
          self.update_enemies(ctx, dt);
      }

      SceneAction::Continue
  }
```
