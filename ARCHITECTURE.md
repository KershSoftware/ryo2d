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
│  │                     Window (winit) + Pixels (wgpu)                    │   │
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
│   2. update(fixed_dt)              Fixed 60Hz updates  │                  │
│      • game.update()               (deterministic)     │                  │
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
│      • Z-sorted sprites            (with interpolation)                   │
│      • Frustum culling                                                    │
│              │                                                            │
│              ▼                                                            │
│   5. pixels.render()               Push to GPU                            │
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
│                             render(alpha)                                 │
├───────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│   1. Get frame buffer from Pixels                                         │
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
│   5. pixels.render() → Push to GPU                                        │
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

## Module Structure

```
src/
├── lib.rs              # Library root, prelude exports
├── main.rs             # Demo game implementation
└── core/
    ├── mod.rs          # Module declarations
    ├── engine.rs       # Main engine, game loop, ApplicationHandler
    ├── game.rs         # Game trait, GameContext
    │
    ├── ecs/            # Entity-Component-System (hecs wrapper)
    │   ├── mod.rs
    │   ├── world.rs    # World wrapper with helper methods
    │   └── components.rs # Transform, Velocity, SpriteRenderer, RenderLayer
    │
    ├── physics.rs      # RigidBody, PhysicsConfig components
    ├── rapier_physics.rs # Rapier2D integration, collision events
    ├── collision.rs    # Collider, ColliderShape, AABB, Circle
    ├── spatial.rs      # SpatialHash for broad-phase (game logic)
    │
    ├── sprite.rs       # Sprite, AnimatedSprite, drawing
    ├── camera.rs       # Camera, coordinate transforms
    ├── input.rs        # InputState, keyboard/mouse handling
    │
    ├── asset.rs        # AssetManager, SpriteHandle, hot reload, async loading
    ├── audio.rs        # AudioManager (Kira), SoundHandle, MusicHandle
    ├── hot_reload.rs   # HotReloadWatcher, filesystem notifications
    │
    └── error.rs        # Error types (EngineError, AssetError, etc.)
```

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `hecs` | High-performance archetype ECS with generational IDs |
| `rapier2d` | Physics simulation, collision detection/response |
| `winit` | Cross-platform windowing and input events |
| `pixels` | 2D pixel buffer rendering via wgpu |
| `kira` | Game audio (sound effects, music, streaming) |
| `image` | Image loading (PNG, JPEG) |
| `notify` | Filesystem watching for hot reload |
| `thiserror` | Ergonomic error types |
| `glam` | Math types (Vec2, Vec3, Mat4) |
| `crossbeam-channel` | Multi-producer channels for Rapier events |

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
