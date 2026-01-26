# Ferrum2D Architecture

## High-Level Overview

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                                 main.rs                                      │
│                            EngineBuilder::new()                              │
│                               .title()                                       │
│                               .resolution()                                  │
│                               .build()                                       │
│                               .run()                                         │
└──────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                                  Engine                                      │
│                                                                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌──────────────────────┐ │
│  │EngineConfig │  │  TimeState  │  │ InputState  │  │        World         │ │
│  │             │  │             │  │             │  │        (ECS)         │ │
│  │ • title     │  │ • delta_time│  │ • keys_down │  │                      │ │
│  │ • width     │  │ • fixed_dt  │  │ • mouse_pos │  │ ┌──────────────────┐ │ │
│  │ • height    │  │ • accumulat.│  │ • scroll    │  │ │     Entities     │ │ │
│  │ • fps       │  │ • frame_cnt │  │             │  │ │  [0, 1, 2, ...]  │ │ │
│  │ • physics_hz│  │             │  │             │  │ └──────────────────┘ │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  │ ┌──────────────────┐ │ │
│                                                     │ │    Components    │ │ │
│  ┌─────────────┐  ┌───────────────────────────────┐ │ │ Transform [...]  │ │ │
│  │   Camera    │  │       Window + Pixels         │ │ │ Velocity [...]   │ │ │
│  │             │  │                               │ │ │ SpriteRender[]   │ │ │
│  │ • position  │  │  winit::Window                │ │ └──────────────────┘ │ │
│  │ • zoom      │  │  pixels::Pixels               │ └──────────────────────┘ │
│  │ • viewport  │  │                               │                          │
│  └─────────────┘  └───────────────────────────────┘                          │
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
│      • Input System                (deterministic)     │                  │
│      • Movement System                                 │                  │
│      • Camera System                                   │                  │
│              │                                         │                  │
│              ▼                                         │                  │
│      accumulator -= fixed_dt ──────────────────────────┘                  │
│              │                                                            │
│              ▼ (exit loop)                                                │
│   3. input.begin_frame()           Clear per-frame input                  │
│              │                                                            │
│              ▼                                                            │
│   4. render(alpha)                 Variable rate rendering                │
│      • Clear background            (with interpolation)                   │
│      • Draw all sprites                                                   │
│              │                                                            │
│              ▼                                                            │
│   5. pixels.render()               Push to GPU                            │
│              │                                                            │
│              ▼                                                            │
│   6. Sleep if ahead                Frame limiting                         │
│                                                                           │
└───────────────────────────────────────────────────────────────────────────┘
```

## ECS (Entity Component System)

```
┌───────────────────────────────────────────────────────────────────────────┐
│                                 World                                     │
│                                                                           │
│  EntityAllocator                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐    │
│  │  next_id: 0 → 1 → 2 → 3 ...                                       │    │
│  │  entities: [Entity(0), Entity(1), Entity(2), ...]                 │    │
│  └───────────────────────────────────────────────────────────────────┘    │
│                                                                           │
│  Component Storage: HashMap<TypeId, ComponentVec>                         │
│  ┌───────────────────────────────────────────────────────────────────┐    │
│  │                                                                   │    │
│  │  TypeId(Transform) ───► RefCell<HashMap<Entity, Transform>>       │    │
│  │                         ┌────────────────────────────────────┐    │    │
│  │                         │ Entity(0) → Transform{x:400,y:300} │    │    │
│  │                         │ Entity(1) → Transform{x:0, y:0}    │    │    │
│  │                         │ Entity(2) → Transform{x:100, y:0}  │    │    │
│  │                         └────────────────────────────────────┘    │    │
│  │                                                                   │    │
│  │  TypeId(Velocity) ────► RefCell<HashMap<Entity, Velocity>>        │    │
│  │                         ┌────────────────────────────────────┐    │    │
│  │                         │ Entity(0) → Velocity{dx:0, dy:0}   │    │    │
│  │                         └────────────────────────────────────┘    │    │
│  │                                                                   │    │
│  │  TypeId(SpriteRenderer) ► RefCell<HashMap<Entity, Sprite...>>     │    │
│  │                         ┌────────────────────────────────────┐    │    │
│  │                         │ Entity(0) → SpriteRenderer{...}    │    │    │
│  │                         │ Entity(1) → SpriteRenderer{...}    │    │    │
│  │                         │ ...                                │    │    │
│  │                         └────────────────────────────────────┘    │    │
│  └───────────────────────────────────────────────────────────────────┘    │
└───────────────────────────────────────────────────────────────────────────┘

Query Pattern:
  world.entities_with_two::<Transform, SpriteRenderer>()
    → Returns [Entity(0), Entity(1), ...] that have BOTH components
```

## Render Pipeline

```
┌───────────────────────────────────────────────────────────────────────────┐
│                             render(alpha)                                 │
└─────────────────────────────────┬─────────────────────────────────────────┘
                                  │
                                  ▼
┌───────────────────────────────────────────────────────────────────────────┐
│  1. Get pixel buffer from Pixels                                          │
│     frame: &mut [u8]  (RGBA, 4 bytes per pixel)                           │
│     [R,G,B,A, R,G,B,A, R,G,B,A, ...]                                      │
│      pixel0   pixel1   pixel2                                             │
└─────────────────────────────────┬─────────────────────────────────────────┘
                                  │
                                  ▼
┌───────────────────────────────────────────────────────────────────────────┐
│  2. Clear background                                                      │
│     for each pixel: set to dark blue (0x1e, 0x40, 0x5e, 0xff)             │
└─────────────────────────────────┬─────────────────────────────────────────┘
                                  │
                                  ▼
┌───────────────────────────────────────────────────────────────────────────┐
│  3. For each entity with (Transform + SpriteRenderer):                    │
│                                                                           │
│     World Position           Camera Transform          Screen Position    │
│     ┌────────────┐           ┌──────────────┐          ┌──────────────┐   │
│     │ x: 400     │  ──────►  │ world_to_    │  ──────► │ screen_x:640 │   │
│     │ y: 300     │           │ screen()     │          │ screen_y:360 │   │
│     └────────────┘           │              │          └──────────────┘   │
│                              │ • subtract   │                             │
│                              │   camera pos │                             │
│                              │ • multiply   │                             │
│                              │   by zoom    │                             │
│                              │ • add half   │                             │
│                              │   viewport   │                             │
│                              └──────────────┘                             │
│                                                                           │
│     4. Frustum Culling: Skip if not visible                               │
│                                                                           │
│     5. sprite.draw_scaled(frame, screen_x, screen_y, zoom)                │
│        • Nearest-neighbor scaling                                         │
│        • Alpha blending                                                   │
│        • Screen clipping                                                  │
└─────────────────────────────────┬─────────────────────────────────────────┘
                                  │
                                  ▼
┌───────────────────────────────────────────────────────────────────────────┐
│  6. pixels.render() → Push buffer to GPU via wgpu                         │
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
Events accumulate    │  keys_pressed     (cleared each frame)     │
between frames  ────►│  keys_released    (cleared each frame)     │
                     │  mouse_delta      (cleared each frame)     │
                     │  scroll_delta     (cleared each frame)     │
                     │                                             │
Persistent state     │  keys_down        (tracks held keys)       │
                     │  mouse_position   (current position)       │
                     └─────────────────────────────────────────────┘
```

## Module Dependencies

```
                                main.rs
                                   │
                      ┌────────────┼────────────┐
                      │            │            │
                      ▼            ▼            ▼
                 engine.rs    camera.rs    ecs/mod.rs
                      │                         │
            ┌─────────┴─────────┐          ┌────┴────┐
            │                   │          │         │
            ▼                   ▼          ▼         ▼
        input.rs            sprite.rs  entity.rs  world.rs
                                           │         │
                                           │         ▼
                                           │    components.rs
                                           │    component.rs
                                           │
                              ┌────────────┴────────────┐
                              │                         │
                              ▼                         ▼
                        External Crates:          External Crates:
                        • winit (windowing)       • image (PNG/JPEG)
                        • pixels (2D rendering)
```

## Coordinate Systems

```
World Space                                Screen Space
(infinite, camera-relative)                (0,0 at top-left)

          ▲ -Y                                   0 ─────────────► X
          │                                      │
          │                                      │
          │      ●───────────●                   │   (after camera transform)
          │      │  Player   │                   │
◄─────────┼──────┤ (400,300) │                   │       ●───────────●
    -X    │      ●───────────●                   │       │  Player   │
          │            ▲                         │       │  (center) │
          │            │                         │       ●───────────●
          │       Camera follows                 │
          ▼ +Y                                   ▼ Y


Zoom affects scale:
  zoom = 1.0:  1 world unit = 1 pixel
  zoom = 2.0:  1 world unit = 2 pixels (zoomed in)
  zoom = 0.5:  1 world unit = 0.5 pixels (zoomed out)
```
