# Bullet Hell - Ryo2D GPU Batching Demo

A bullet hell survival game showcasing the Ryo2D engine's GPU sprite batching capabilities with hundreds of bullets and particle effects.

## Demo Purpose

This example demonstrates:
- **GPU Sprite Batching** - Efficiently rendering hundreds of sprites per frame
- **Particle Systems** - Explosion effects with clustered particles
- **Dynamic Spawning** - Procedural bullet patterns from wall turrets
- **Collision Detection** - Player vs bullets, bullets vs platforms
- **ECS Architecture** - Clean component-based entity management

## Gameplay

Survive as long as possible while dodging bullets fired from turrets mounted on the left and right walls. Hide behind platforms to avoid incoming fire. When bullets hit anything, they explode into particle clusters that spread out - perfect for stress-testing GPU batching!

### Objective
- Survive as long as you can
- Beat your best survival time
- Test how many entities the GPU batching can handle

### Features
- **Player Controls** - WASD/Arrow keys + Space to jump
- **3 Lives System** - Temporary invincibility after taking damage
- **Wall Turrets** - Move up/down and shoot bullet bursts
- **Platform System** - Jump between platforms to avoid bullets
- **Particle Explosions** - Bullets explode into 12 particles on impact
- **Live Entity Counter** - See bullet/particle counts in real-time

## Controls

- **WASD / Arrow Keys** - Move left/right
- **Space / W** - Jump
- **ESC** - Pause/return to menu
- **--debug / -d** flag - Show frame timing overlay

## Running the Game

### Using Cargo

```bash
# From the bullet-hell directory
cargo run

# With debug overlay
cargo run -- --debug
```

### Using the Ryo2D CLI

```bash
# From the ryo2d project root
./run bullet-hell

# From the bullet-hell directory
ryo2d dev
# or
ryo2d build && ryo2d run
```

## Implementation Highlights

### GPU Batching
The game creates hundreds of sprites per frame (bullets + particles). The engine's GPU batching system automatically groups sprites by texture and renders them in optimized batches, minimizing draw calls.

### Component Architecture
- `Player` - Lives, invincibility, grounded state
- `Turret` - Side, movement, shooting timers
- `Bullet` - Velocity vector for movement
- `Particle` - Velocity, lifetime, alpha fade
- `Platform` - Static collision geometry
- `Velocity` - Reusable movement component

### Collision Systems
- Player vs platforms (landing on top)
- Bullets vs player (circle collision)
- Bullets vs platforms (AABB collision)
- Explosion spawning on any bullet collision

### Performance Notes
The game tracks and displays:
- Active bullet count
- Active particle count
- Total entities being batched

Typical gameplay sees 50-200 bullets and 100-500 particles simultaneously, all rendered in a few GPU draw calls thanks to sprite batching.

## Assets

Simple colored PNG sprites:
- `player.png` - 24x24 cyan square
- `turret.png` - 32x32 red square
- `bullet.png` - 8x8 yellow square
- `particle.png` - 4x4 orange square
- `platform.png` - 100x20 gray rectangle

## Game Design

The game is intentionally simple to focus on demonstrating GPU batching:
- Fixed arena with platforms
- Procedural bullet patterns aimed at player
- Particle explosions create bursts of entities
- Entity counts displayed for performance testing

This makes it easy to see how the engine handles hundreds of dynamic sprites without performance degradation.
