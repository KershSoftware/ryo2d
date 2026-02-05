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
│  │ • fps       │  │ • time_scale│  │ • gamepad   │  │                      │ │
│  │ • physics_hz│  │ • hitstop   │  │             │  │                      │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └──────────────────────┘ │
│                                                                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌──────────────────────┐ │
│  │   Camera    │  │AssetManager │  │AudioManager │  │ RapierPhysicsWorld   │ │
│  │             │  │             │  │             │  │                      │ │
│  │ • position  │  │ • sprites   │  │ • sfx       │  │ • RigidBodySet       │ │
│  │ • zoom      │  │ • hot reload│  │ • music     │  │ • ColliderSet        │ │
│  │ • shake     │  │ • caching   │  │ • volume    │  │ • PhysicsPipeline    │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └──────────────────────┘ │
│                                                                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌──────────────────────┐ │
│  │   Events    │  │EffectsManager│ │  DevLog     │  │   Dev Windows        │ │
│  │             │  │             │  │             │  │                      │ │
│  │ • pub/sub   │  │ • fade/crt  │  │ • circular  │  │ • Profiler (F10)     │ │
│  │ • 1-frame   │  │ • vignette  │  │ • levels    │  │ • Logger (F9)        │ │
│  │ • type-safe │  │ • flash     │  │ • modules   │  │ • Flamegraph         │ │
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
│   3. input.begin_frame()           Clear per-frame input                  │
│              │                                                            │
│              ▼                                                            │
│   4. render(alpha)                 Variable rate rendering                │
│      • game.render()                                                      │
│      • game.render_gpu()                                                  │
│      • game.render_ui()                                                   │
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
│                                                                           │
│  Generational IDs: Entity = (index: u32, generation: u32)                 │
│  Stale handles safely return None instead of accessing wrong entity       │
│                                                                           │
│  World Wrapper Features:                                                  │
│  • spawn/despawn with generation tracking                                 │
│  • Parent/Children hierarchy with WorldTransform propagation              │
│  • spawn_child(), set_parent(), despawn_recursive()                       │
│  • Generation counter for scene clear detection                           │
└───────────────────────────────────────────────────────────────────────────┘
```

### Built-in Components

| Category | Components |
|----------|------------|
| **Transform** | Transform (position, rotation, scale, pivot), Velocity, WorldTransform |
| **Rendering** | Renderable (sprite/tile/shader), Material, SpriteShader, RenderLayer |
| **Hierarchy** | Parent, Children |
| **Animation** | Tween (24 easing functions), Knockback, SquashStretch, YSort |
| **Pooling** | Active, Inactive (engine skips Inactive in render/physics/animation) |

### Object Pooling

Pre-allocated entity pools for high-frequency spawning (bullets, particles):
- `Pool<B>` generic over component bundle type
- `PoolConfig`: initial_capacity, max_capacity, expand_batch_size, exhausted_policy
- Automatic Active/Inactive marker swapping
- World generation tracking for scene clear safety
- Engine auto-skips Inactive entities in all systems

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
│  │ (no Static)     │  │ Restitution     │  │ Constraint solving      │    │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────┘    │
│                                                                           │
│  Entity ↔ Handle Mapping (bidirectional)                                  │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  entity_to_body / body_to_entity                                   │   │
│  │  entity_to_collider / collider_to_entity                           │   │
│  │  Pixel ↔ Meter: PIXELS_PER_METER = 100.0                           │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Collision Events (via ChannelEventCollector)                             │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  PhysicsCollisionEvent { entity_a, entity_b, Started/Stopped }     │   │
│  │  Helper methods: involves(entity), other(entity)                   │   │
│  └────────────────────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────────────────────┘
```

### Physics Loop

1. Auto-register new entities (RigidBody + Collider + Transform)
2. `sync_kinematic_from_ecs()` - Update kinematic positions BEFORE step
3. `sync_velocities_from_ecs()` - Apply impulses from game code
4. `physics.step(gravity)` - Rapier simulates
5. `sync_to_ecs()` - Copy positions/velocities FROM Rapier TO ECS
6. Collision events available via `ctx.collision_events`

### Dual Physics Systems

| System | Purpose | Use Case |
|--------|---------|----------|
| **RapierPhysicsWorld** | Full dynamics (gravity, forces, impulses) | Platformers, physics puzzles |
| **PhysicsWorld** | Spatial hash + collision detection only | Simple overlap checks, triggers |

### Spatial Partitioning

Grid-based broad-phase (`SpatialHash`):
- Default cell size: 64×64 pixels
- Entities placed in all overlapping cells
- Reduces narrow-phase from O(n²) to O(n)
- Automatic cleanup of empty cells

## Render Pipeline

Ryo2D uses a **pass-based render pipeline** where each phase is a `RenderPass` with declared dependencies.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                           RenderPipeline                                  │
│                                                                           │
│  Pass Execution Order (topological sort by dependencies):                 │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │                                                                    │   │
│  │   ClearPass ──► ParallaxPass ──► CpuSpritePass ──► UploadPass      │   │
│  │                                                          │         │   │
│  │                                                          ▼         │   │
│  │   PresentPass ◄── EffectsPass ◄── UiPass ◄── LightingPass          │   │
│  │                                                    │               │   │
│  │                                                    ▼               │   │
│  │                                              GpuSpritePass         │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  Built-in Passes:                                                         │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │  clear       │  Clear pixel buffer to background color             │   │
│  │  parallax    │  GPU-rendered parallax background layers            │   │
│  │  cpu_sprite  │  Software sprite rendering to pixel buffer          │   │
│  │  upload      │  Upload CPU pixel buffer to GPU texture             │   │
│  │  gpu_sprite  │  Batched GPU sprite rendering (10k max instances)   │   │
│  │  trail       │  Motion trail rendering (Line2D, Trail2D)           │   │
│  │  lighting    │  2D lighting with point lights and shadows          │   │
│  │  ui          │  User interface overlay                             │   │
│  │  effects     │  Post-processing (CRT, vignette, fade, flash)       │   │
│  │  present     │  Final output to swapchain                          │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                           │
│  RenderContext (shared state for all passes):                             │
│  • device, queue, encoder (wgpu resources)                                │
│  • game_texture, scratch_a/b (ping-pong buffers)                          │
│  • pixel_buffer (CPU software rendering)                                  │
│  • camera, sprite_gpu, batch_collector                                    │
│  • trail_renderer, lighting, effects                                      │
└───────────────────────────────────────────────────────────────────────────┘
```

### GPU Sprite Batching

- Automatic grouping by (texture_id, has_effects)
- Z-order sorting across batches
- 112 bytes per instance (transform matrix, UV, tint, effect params)
- Instance buffer pre-upload before render pass
- Two pipelines: base (no effects) and effect (per-sprite shaders)

### Per-Sprite Shader Effects (11 built-in)

Outline, Glow, Flash, ColorShift, Silhouette, Pixelate, Wave, Dissolve, Invert, Grayscale, DirectionalOutline

### Post-Processing Effects

| Effect | Description |
|--------|-------------|
| Fade | Screen fade to color (scene transitions) |
| ColorTint | Color grading/mood (sepia, night vision) |
| Vignette | Darkened edges (cinematic look) |
| ScreenFlash | One-shot flash with auto-decay (damage) |
| CRT | Scanlines, curvature, RGB offset (retro) |
| ChromaticAberration | RGB color fringing (glitch/lens) |

## Input System

```
┌──────────────────┐      ┌──────────────────┐      ┌──────────────────┐
│   winit Events   │─────►│   InputState     │─────►│  Game Systems    │
└──────────────────┘      └──────────────────┘      └──────────────────┘

                     ┌─────────────────────────────────────────────┐
                     │             Per-Frame State                 │
                     │                                             │
Events accumulate    │  keys_pressed/released  (cleared each frame)│
between frames  ────►│  mouse_delta            (cleared each frame)│
                     │  scroll_delta           (cleared each frame)│
                     │                                             │
Persistent state     │  keys_down              (tracks held keys)  │
                     │  mouse_position         (current position)  │
                     │  gamepad_state          (gilrs integration) │
                     └─────────────────────────────────────────────┘
```

## Event Bus

Type-safe publish-subscribe system for decoupled inter-system communication:

- Events exist for exactly ONE frame (cleared at frame start)
- Any `'static + Send + Sync` type can be an event
- Multiple readers can consume same events (no ownership transfer)

### Built-in Events

| Event | Source | Purpose |
|-------|--------|---------|
| CollisionStarted/Stopped | Physics | Collision state changes |
| TweenStarted/Completed | Tweening | Animation lifecycle |
| ClientConnected/Disconnected | Network | Multiplayer (host) |
| ConnectedToServer/DisconnectedFromServer | Network | Multiplayer (client) |
| AssetReloaded/AssetLoadFailed | Assets | Hot reload status |

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
│  Actions: Push, Pop, Replace, Clear                                       │
│  Lifecycle: on_enter → update/render → on_exit                            │
└───────────────────────────────────────────────────────────────────────────┘
```

## Audio System

Powered by [Kira](https://github.com/SoftbearStudios/kira) for professional game audio.

### Volume Hierarchy (Multiplicative)

`Final Volume = master × category × sound`

- Master volume: affects ALL audio
- Category volumes: SFX vs Music (separate control)
- Per-sound volume: passed in play options

### Capabilities

| Feature | SFX | Music |
|---------|-----|-------|
| Format | WAV, MP3, OGG, FLAC | Same |
| Loading | Cached in memory | Streamed from disk |
| Control | SoundHandle (pause, resume, stop) | MusicHandle (same + fades) |
| Fading | No | fade_in, fade_to, fade_out, crossfade |
| Positioning | Panning (-1.0 to 1.0) | N/A |
| Pitch | Playback rate | Playback rate |

## UI System

Hybrid approach: **immediate mode** for simple elements, **retained mode** for stateful widgets.

```
┌───────────────────────────────────────────────────────────────────────────┐
│                          Hybrid UI System                                 │
│                                                                           │
│  Immediate Mode (stateless):           Retained Mode (stateful):          │
│  ┌─────────────────────────────┐       ┌─────────────────────────────┐    │
│  │  button(), label(), slider  │       │  TextInputState             │    │
│  │  checkbox(), toggle()       │       │  DropdownState              │    │
│  │  progress_bar()             │       │  ScrollViewState            │    │
│  │  draw_rect/text/sprite      │       │  FocusTrap                  │    │
│  └─────────────────────────────┘       └─────────────────────────────┘    │
│                                                                           │
│  Layout Containers:                    Theme System:                      │
│  ┌─────────────────────────────┐       ┌─────────────────────────────┐    │
│  │  HBox (horizontal + valign) │       │  Centralized widget styles  │    │
│  │  VBox (vertical + halign)   │       │  Theme::dark(), light()     │    │
│  │  Stack (layered/split)      │       │  Hot-reload from JSON       │    │
│  └─────────────────────────────┘       └─────────────────────────────┘    │
│                                                                           │
│  Focus Management:                     UI Tweening:                       │
│  ┌─────────────────────────────┐       ┌─────────────────────────────┐    │
│  │  Focusable trait            │       │  TweenFloat, TweenVec2      │    │
│  │  Tab/Shift+Tab navigation   │       │  TweenColor, TweenRect      │    │
│  │  FocusTrap for modals       │       │  24 easing functions        │    │
│  └─────────────────────────────┘       └─────────────────────────────┘    │
└───────────────────────────────────────────────────────────────────────────┘
```

### Clipping & Rendering

- Scissor rect stack (max 16 levels, auto-intersection)
- `with_no_clip()` for temporary bypass
- `draw_modal_overlay()` for dimming backgrounds
- `draw_focus_ring()` for keyboard navigation feedback

## Save System

Three-tier save architecture with compression:

| Tier | Purpose | File Pattern |
|------|---------|--------------|
| SystemState | Engine settings (audio, display) | `system.ryo-system` |
| WorldState | ECS world snapshots | `{slot}.ryo-world` |
| GameSaveData<T> | Custom game data (generic) | `{slot}.ryo-save` |

**Format**: Magic header (`RYO2`) + version + DEFLATE-compressed RON data

## Dev Tools

### Debug Keys

| Key | Action |
|-----|--------|
| F1 | Cycle overlay: Off → FPS → Frame Timing → Full Stats |
| F2 | Toggle collision boundaries |
| F3 | Toggle velocity vectors |
| F4 | Toggle origin points |
| F5 | Toggle borderless fullscreen |
| F9 | Toggle Logger window |
| F10 | Toggle Profiler window (requires `profiler` feature) |

### Profiler Window (F10)

Real-time flamegraph viewer using [puffin](https://github.com/EmbarkStudios/puffin):
- Hierarchical scope display (Game, Engine-CPU, Engine-GPU sections)
- Color-coded bars by scope name
- Hover tooltips with timing
- Pause/resume for analysis
- Smooth frame duration display

### Logger Window (F9)

Live log viewer with filtering:
- 5 log levels: Trace, Debug, Info, Warn, Error
- Context filtering: Engine, Game, External
- ISO timestamps + elapsed time
- Click-drag text selection + Ctrl+C copy
- Auto-scroll with pause detection
- Circular buffer (10,000 max entries)

### Dev Logging

Thread-safe global log buffer with macros:
- `engine_info!()`, `game_warn!()`, etc.
- Optional module tagging: `engine_info!(module: "Physics", "message")`
- Frame number tracking

## CLI System

```
┌───────────────────────────────────────────────────────────────────────────┐
│                        ryo2d CLI Commands                                 │
│                                                                           │
│  ryo2d init [name]     Create new project with full structure             │
│  ryo2d build           Build optimized release bundle                     │
│  ryo2d run             Execute release build                              │
│  ryo2d dev [-p]        Development mode (-p enables profiler)             │
│  ryo2d export          Package release into distributable zip             │
│  ryo2d install-skills  Install 15 Agent Skills to .github/skills/         │
│                                                                           │
│  Features:                                                                │
│  • Workspace discovery (searches up to 3 parents for Ryo.toml)            │
│  • Shared target directory for workspace builds                           │
│  • Dev profile optimization check with auto-fix prompt                    │
│  • Config embedding via RYO_EMBED_* environment variables                 │
│  • Windows icon embedding via build.rs                                    │
└───────────────────────────────────────────────────────────────────────────┘
```

## Workspace Configuration (Ryo.toml)

| Section | Purpose | Runtime |
|---------|---------|---------|
| `[project]` | Name, title, version | Build-time |
| `[paths]` | entry_point, assets, save_data | Build-time |
| `[system]` | Resolution, vsync, volumes, show_fps (defaults for system.json) | User-modifiable |
| `[debug]` | Debug flags, profiler, logger | Dev-only |
| `[release]` | Icon path | Build-time |
| `[env]` | Game-specific key-value pairs | Game code |

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

Rapier Space: PIXELS_PER_METER = 100.0 (500 pixels = 5 meters)
```

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `hecs` | Archetype ECS with generational IDs |
| `rapier2d` | Physics simulation, collision detection |
| `winit` | Cross-platform windowing and input |
| `wgpu` | GPU abstraction for rendering |
| `kira` | Game audio (SFX, music, streaming) |
| `puffin` | Hierarchical profiling (optional) |
| `glam` | Math types (Vec2, Vec3, Mat4) |
| `image` | Image loading (PNG, JPEG) |
| `notify` | Filesystem watching for hot reload |
| `ron` | Save data serialization |
| `flate2` | Save file compression |
| `toml` | Configuration parsing |
| `gilrs` | Gamepad input |
| `arboard` | Clipboard integration |

## Networking

Client-server multiplayer with Listen Server support. See **[NETWORK_ARCHITECTURE.md](./NETWORK_ARCHITECTURE.md)** for complete documentation.

**Quick Overview:**
- **Transport**: UDP via renet (3 channels: Unreliable, ReliableOrdered, ReliableUnordered)
- **Server Types**: NetworkServer (dedicated), ListenServer (player-hosted)
- **Replication**: Generation-based delta sync with NetworkEntity, Authority, DirtyTracker
- **RPC**: Type-safe remote procedure calls with RpcRegistry

```
┌─────────────────┐         ┌─────────────────┐
│  ListenServer   │◄──UDP──►│  NetworkClient  │
│  (host + serve) │         │   (players)     │
└────────┬────────┘         └────────┬────────┘
         │                           │
         ▼                           ▼
┌─────────────────────────────────────────────┐
│  ReplicationManager + RpcRegistry           │
│  (entity sync)         (game events)        │
└─────────────────────────────────────────────┘
```

## Additional Systems

| System | Location | Purpose |
|--------|----------|---------|
| Animation | `animation.rs` | Animation, Animator, AnimationController |
| Camera Shake | `camera.rs` | Trauma-based screen shake |
| Time Control | `engine.rs` | Slow-mo, hitstop (time_scale, hitstop) |
| Parallax | `parallax.rs` | Layered scrolling backgrounds |
| Tiled Rendering | `tile.rs` | TiledSprite, CompositeTile, GroupedCollider |
| Cursor | `cursor.rs` | 18 system cursor styles, visibility, confinement |
| State Machine | `state_machine/` | Generic FSM utilities |
| Proc Gen | `procgen/` | Procedural generation helpers |
| Navigation | `navigation/` | Pathfinding utilities |
| Networking | `network/` | Multiplayer (see [NETWORK_ARCHITECTURE.md](./NETWORK_ARCHITECTURE.md)) |
