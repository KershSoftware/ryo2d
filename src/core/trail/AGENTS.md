# AGENTS.md - Trail & Line Rendering System

## Overview

This module provides GPU-accelerated trail and line rendering for visual effects like projectile trails, sword swipes, laser beams, and debug visualization.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports and documentation |
| `trail.rs` | `Trail2D` component - automatic position history recording |
| `line.rs` | `Line2D` component - explicit polyline/curve rendering |
| `renderer.rs` | `TrailRenderer` - GPU batch rendering with triangle strips |

## Core Components

### Trail2D

Automatically records position history for moving entities. Call `update(x, y, dt)` each frame.

**Presets:**
| Preset | Use Case | Characteristics |
|--------|----------|-----------------|
| `Trail2D::bullet(color)` | Projectiles | Short, fast-fading, tapered |
| `Trail2D::particle(color)` | Sparks, particles | Thin, very short |
| `Trail2D::swipe(color, width)` | Sword slashes | Wide, tapers to point |
| `Trail2D::laser(color, width)` | Laser beams | Consistent width, no fade |
| `Trail2D::fading_laser(color, width, fade_time)` | Laser fade-out effects | Time-based fade for ghost effects |
| `Trail2D::lightning(color, width)` | Electrical arcs | Glow + Lightning effect + Round cap |
| `Trail2D::fire(color, width)` | Flame/smoke | Soft + Noise effect + tapered + color fade |
| `Trail2D::energy(color, width)` | Energy beams | Glow + UvScroll effect + no taper |

**Key Methods:**
```rust
Trail2D::new(color, width)       // Create custom trail
  .with_length(max_points)       // Max history (default: 20)
  .with_taper(ratio)             // End width ratio (0.0 = point)
  .with_fade(bool)               // Position-based alpha fade (head to tail)
  .with_lifetime(seconds)        // Time-based fade + auto-removal
  .with_end_color(color)         // Gradient to end color
  .with_min_distance(pixels)     // Min distance between points
  .with_style(TrailStyle)          // Per-trail rendering style
  .with_effect(TrailEffect)        // GPU displacement effect
  .with_cap(LineCap)               // Endpoint cap style
  .with_join(LineJoin)             // Corner join style (Line2D primarily)

trail.clear();                   // Reset trail
trail.set_enabled(bool);         // Enable/disable recording (aging continues when disabled)
trail.age_alpha(age)             // Get fade multiplier for point age

// Manual point population (for static effects like laser fade-out)
trail.push_point(x, y);          // Add point at front (newest)
trail.push_point_back(x, y);     // Add point at back (oldest)
trail.populate_line(x1, y1, x2, y2, num_points); // Fill with line segment
```

**Fade Modes:**
| Mode | Method | Description |
|------|--------|-------------|
| Position-based | `.with_fade(true)` | Fades based on index position (0 to max_points) |
| Time-based | `.with_fade(false).with_lifetime(secs)` | Fades based on point age, removes old points automatically |
| Distance-based | `.with_fade_distance(pixels)` | Fades based on actual distance in pixels from head |
| None | `.with_fade(false)` | No fading, solid trail |

**Position vs Distance fade:**
- Position fade: fades at array index `i / max_points` - fast objects have stretched fades
- Distance fade: fades at `distance / fade_distance` - consistent fade regardless of speed

**Note:** The engine automatically calls `trail.update()` each frame using the entity's Transform position. Manual calls are only needed for custom positions.

### Line2D

Explicitly defined lines, shapes, and curves. Points are set directly, not recorded.

**Shape Constructors:**
```rust
Line2D::segment(start, end, color, width)
Line2D::rect(x, y, w, h, color, width)
Line2D::circle(center, radius, color, width, segments)
Line2D::arc(center, radius, start_angle, end_angle, color, width, segments)
Line2D::arrow(start, end, color, width, head_size)
```

**Curve Constructors:**
```rust
Line2D::quadratic_bezier(start, control, end, color, width, segments)
Line2D::bezier(start, c1, c2, end, color, width, segments)  // Cubic
```

**Key Methods:**
```rust
  .with_style(TrailStyle)          // Per-line rendering style
  .with_cap(LineCap)               // Endpoint cap style
  .with_join(LineJoin)             // Corner join style
```

**Dynamic Lines:**
```rust
let mut line = Line2D::default();
line.push_point(Vec2::new(x, y));  // Add point
line.pop_point();                   // Remove last
line.clear();                       // Clear all
line.set_points(vec![...]);         // Replace all
```

### TrailRenderer

GPU batch renderer using triangle strips. Handles both Trail2D and Line2D components.

**Rendering Styles:**
| Style | Description |
|-------|-------------|
| `TrailStyle::Solid` | Sharp edges, uniform color |
| `TrailStyle::Soft` | Soft, fading edges |
| `TrailStyle::Glow` | Glowing effect with bright core |

### GPU Effects (TrailEffect)

GPU-driven effects applied via vertex shader displacement. Set per-trail with `.with_effect()`.

| Effect | Parameters | Description |
|--------|-----------|-------------|
| `Lightning { jitter, speed, detail }` | jitter: max displacement px, speed: re-randomization Hz, detail: noise octaves (1-4) | Jagged electrical arc displacement |
| `Noise { frequency, amplitude, speed }` | frequency: spatial freq, amplitude: max displacement px, speed: animation scroll | Smooth organic distortion |
| `UvScroll { speed }` | speed: UV units/sec | Scrolling texture animation |
| `Ribbon { amplitude, frequency, speed }` | amplitude: wave height px, frequency: waves per length, speed: animation speed | Sine wave ribbon undulation |

**Usage example:**
```rust
let bolt = Trail2D::new(Color::CYAN, 6.0)
    .with_style(TrailStyle::Glow)
    .with_effect(TrailEffect::Lightning { jitter: 20.0, speed: 8.0, detail: 3 })
    .with_cap(LineCap::Round);
```

### Per-Trail Style

Each Trail2D/Line2D stores its own `TrailStyle`. Set with `.with_style()`:
- `TrailStyle::Solid` - Sharp edges, SDF anti-aliased (default)
- `TrailStyle::Soft` - Fading edges
- `TrailStyle::Glow` - Bright core with falloff

Previously, style was a global parameter on the renderer. Now it's per-vertex data, allowing mixed styles in a single draw call.

### Line Caps (LineCap)

Endpoint geometry for trails and lines. Set with `.with_cap()`:

| Cap | Description |
|-----|-------------|
| `Butt` (default) | Flat cut at endpoint |
| `Round` | Semicircle at endpoints (~8 triangles per cap) |
| `Square` | Extends by half-width beyond endpoint |

### Line Joins (LineJoin)

Corner geometry for polylines (primarily Line2D). Set with `.with_join()`:

| Join | Description |
|------|-------------|
| `None` (default) | No join handling (fastest) |
| `Miter` | Sharp corners (miter limit 4.0, falls back to bevel) |
| `Bevel` | Flat diagonal cut at corners |
| `Round` | Smooth rounded corners |

## Usage Patterns

### Adding Trails to Entities

```rust
// Spawn entity with trail - engine auto-updates using Transform position
let bullet = world.spawn((
    Transform::new(x, y),
    Velocity::new(500.0, 0.0),
    Renderable::sprite(sprite),
    Trail2D::bullet(Color::YELLOW),
));

// With time-based fade (points fade and are removed after 0.5 seconds)
let missile = world.spawn((
    Transform::new(x, y),
    Trail2D::new(Color::ORANGE, 6.0)
        .with_length(50)
        .with_fade(false)
        .with_lifetime(0.5),
));
```

### Static Lines/Shapes

```rust
// Debug visualization
let bounds = world.spawn((
    Line2D::rect(0.0, 0.0, 800.0, 600.0, Color::GREEN, 2.0),
));

// Laser beam (updates each frame)
fn update_laser(&mut self) {
    if let Some(mut line) = world.get_component_mut::<Line2D>(self.laser) {
        line.set_points(vec![self.gun_pos, self.target_pos]);
    }
}
```

### Laser Fade-Out Effects

**Option 1: Static Fade** - Entire line fades uniformly:

```rust
// When laser stops firing, spawn a fading trail
let mut fade_trail = Trail2D::fading_laser(Color::RED, 8.0, 0.3);
fade_trail.populate_line(start_x, start_y, end_x, end_y, 30);
fade_trail.set_enabled(false); // Stop recording, just fade out

// Spawn without Transform - trail stores absolute positions
let entity = world.spawn((fade_trail,));
```

**Option 2: Moving Tracer** - Dynamic "sweep" effect (recommended):

```rust
// Spawn fast-moving invisible entity that leaves a fading trail
let trail = Trail2D::new(Color::RED, 8.0)
    .with_length(60)
    .with_fade(true)
    .with_lifetime(0.3)
    .with_min_distance(8.0);

let tracer = world.spawn((
    Transform::new(start_x, start_y),
    trail,
    LaserTracer::new(end_x, end_y, 3000.0), // Custom marker component
));

// In update loop: move tracer toward target
// When reached: set_enabled(false) and remove marker
// Cleanup: despawn when trail.point_count() == 0
```

The moving tracer creates a more dynamic "retraction" effect where the laser visually sweeps away rather than uniformly fading.

**Key Points:**
- `set_enabled(false)` stops new point recording but aging/removal continues
- Points are removed when they exceed `lifetime`
- Trails without Transform still render and age correctly
- Use `populate_line()` for static effects, moving tracer for dynamic effects

### Drawing Mechanics

```rust
// Player drawing with mouse
fn on_mouse_move(&mut self, x: f32, y: f32) {
    if self.is_drawing {
        self.drawing_line.push_point(Vec2::new(x, y));
    }
}
```

## Integration with Engine

The `TrailRenderer` is instantiated by the engine and renders all `Trail2D` and `Line2D` components automatically after sprite rendering.

**Rendering Order:**
1. Sprites (by z_order)
2. Trails and Lines (by z_order)
3. UI

## Performance Notes

- **Batching**: All trails/lines rendered in single draw call per texture group
- **Vertex Size**: 56 bytes per vertex (up from 32) to accommodate style/effect data
- **Pipelines**: 2 render pipelines (down from 6) - textured and untextured
- **Mixed Rendering**: Mixed styles and effects render in a single draw call per texture group
- **Vertex Limit**: 65,536 vertices per frame (shared across all trails/lines)
- **Point Count**: More points = more vertices = higher GPU cost
- **Min Distance**: Use `with_min_distance()` to prevent clustering on fast objects
- **Disable**: Set `trail.set_enabled(false)` when off-screen

## Constants

```rust
pub const MAX_TRAIL_POINTS: usize = 128;    // Max points per trail
pub const MAX_TRAIL_VERTICES: usize = 65536; // Max vertices per frame
```

## Shader

The trail system uses 2 uber-shaders (`shaders/core/trail.wgsl` and `trail_textured.wgsl`) that handle:
- Per-vertex style branching (Solid/Soft/Glow) with SDF anti-aliasing
- GPU effects: Lightning, Noise, UV Scroll, Ribbon (vertex shader displacement)
- Effect parameters packed into vertex data (56-byte vertex format)

Previously 6 pipelines (3 styles x 2 texture modes). Now 2 pipelines (textured + untextured).

## Tips for AI Agents

1. **Trail vs Line**: Use `Trail2D` for moving objects (auto-records), `Line2D` for static/explicit shapes
2. **Auto-Update**: Engine automatically updates trails using entity's Transform - no manual `update()` needed
3. **Presets First**: Try presets before customizing (`bullet`, `particle`, `swipe`, `laser`, `fading_laser`, `lightning`, `fire`, `energy`)
4. **Memory**: Trails use `VecDeque` - old points removed when `max_points` exceeded or `lifetime` reached
5. **Fade Modes**: Position fade (`.with_fade(true)`) for fast objects, Time fade (`.with_lifetime()`) for slow/stationary
6. **Visibility**: Lines have `visible` flag, Trails have `enabled` flag
7. **Fade-Out Effects**: Use `fading_laser` + `populate_line()` + `set_enabled(false)` for ghost/fade effects
8. **No Transform OK**: Trails without Transform still render - useful for standalone fade-out effects
9. **Effects**: Use presets (`lightning`, `fire`, `energy`) before building custom effects
10. **Style per-trail**: Each trail/line stores its own style — no need to change the global renderer style
11. **Caps/Joins**: Use `LineCap::Round` + `LineJoin::Round` for polished visuals on Line2D shapes
