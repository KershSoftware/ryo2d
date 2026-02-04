# Game Loop

ryo2d uses a **fixed timestep** game loop pattern, which is the industry standard for deterministic game physics.

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

## Render Interpolation

When render FPS exceeds physics update rate (e.g., 144 FPS render with 60 Hz physics), entities can appear to stutter. Render interpolation smooths this by lerping between the previous and current transform based on how far we are into the next physics step.

### Enabling Interpolation

Interpolation is opt-in per entity via the `Renderable` component:

```rust
// Enable interpolation for smooth motion
ctx.world.spawn((
    Transform::new(x, y),
    Velocity::new(100.0, 0.0),
    Renderable::sprite(sprite).interpolated(),  // <-- Enable here
));
```

### When to Use

**Good candidates:**
- Fast-moving entities (balls, bullets, projectiles)
- Player-controlled entities
- Scrolling backgrounds
- Anything where stuttering is noticeable

**Skip interpolation for:**
- Static entities (walls, platforms)
- Short-lived particles
- UI elements

### Handling Teleports

When you instantly move an entity (teleport), call `skip_interpolation` to prevent visual sliding from the old position:

```rust
// Teleport entity to new position
if let Some(mut t) = ctx.world.get_component_mut::<Transform>(entity) {
    t.x = new_x;
    t.y = new_y;
}

// Skip interpolation this frame to avoid sliding effect
InterpolationSystem::skip_interpolation(&mut ctx.world, entity);
```

### Accessing Timing Values

The `TimeInfo` struct provides all timing values for advanced use cases:

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // Access timing info
    let time = ctx.time;

    time.fixed_dt;      // Fixed timestep (e.g., 1/60)
    time.scaled_dt;     // dt with time_scale applied
    time.real_dt;       // Actual frame delta (unscaled)
    time.render_alpha;  // Interpolation factor (0.0 - 1.0)
    time.time_scale;    // Current time scale
    time.is_hitstop;    // True during hitstop
    time.frame_count;   // Total frames elapsed
}
```
