# Game Loop

Ferrum2D uses a **fixed timestep** game loop pattern, which is the industry standard for deterministic game physics.

## The Pattern

```
┌─────────────────────────────────────────────────────┐
│                    Frame Start                      │
├─────────────────────────────────────────────────────┤
│  Accumulate time since last frame                   │
├─────────────────────────────────────────────────────┤
│  While (accumulated_time >= fixed_timestep):        │
│    ├── Run update() with fixed dt (e.g., 1/60s)     │
│    └── Subtract fixed_timestep from accumulator     │
├─────────────────────────────────────────────────────┤
│  Render with interpolation alpha                    │
├─────────────────────────────────────────────────────┤
│  Wait for next frame (FPS limiting)                 │
└─────────────────────────────────────────────────────┘
```

## Why Fixed Timestep?

1. **Deterministic physics** - Same input = same output, regardless of frame rate
2. **Stable simulation** - No "explosion" from large delta times
3. **Spiral of death protection** - Caps updates per frame to prevent freezing

## Your Update Function

Your `Game::update()` receives a fixed `dt` value (e.g., 1/60 = 0.01667 seconds):

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // dt is always the same value (fixed timestep)
    // Use it for movement: position += velocity * dt
}
```

The engine handles applying velocity to transforms automatically.

## Configuration

```rust
EngineBuilder::new()
    .fps(60)           // Target frame rate
    .physics_hz(60)    // Fixed update rate
```
