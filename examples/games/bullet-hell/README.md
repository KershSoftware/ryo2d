# Bullet Hell

![Bullet Hell Demo](../../../docs/assets/bullet-hell.gif)

A survival shooter stress-testing GPU sprite batching with hundreds of entities.

## Features Demonstrated

- **GPU Sprite Batching** - Hundreds of sprites rendered in minimal draw calls
- **Particle Systems** - Explosion effects with clustered particles
- **Dynamic Spawning** - Procedural bullet patterns from wall turrets
- **Collision Detection** - Player vs bullets, bullets vs platforms
- **Live Entity Counter** - Real-time bullet/particle counts for performance testing

## Controls

- `WASD` / Arrow Keys - Move
- `Space` / `W` - Jump
- `Escape` - Pause

## Run

```bash
ryo2d dev          # Development mode
ryo2d build && ryo2d run  # Production build
```
