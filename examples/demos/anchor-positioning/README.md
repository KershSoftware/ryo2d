# Anchor Positioning Demo

Demonstrates the **Anchor** system for sprite positioning in Ryo2D.

## What It Shows

A 3x3 grid where each cell demonstrates a different anchor point:

```
┌─────────────┬─────────────┬─────────────┐
│  TopLeft    │  TopCenter  │  TopRight   │
├─────────────┼─────────────┼─────────────┤
│ CenterLeft  │   Center    │ CenterRight │
├─────────────┼─────────────┼─────────────┤
│ BottomLeft  │BottomCenter │ BottomRight │
└─────────────┴─────────────┴─────────────┘
```

Each cell shows:
- A **red crosshair** marking the target position (Transform x, y)
- A **blue rectangle** positioned using that anchor

The anchor determines which part of the sprite aligns with the crosshair.

## Controls

| Key | Action |
|-----|--------|
| Arrow keys | Adjust global offset for all sprites |
| R | Reset offset to zero |
| Space | Toggle offset display |
| ESC | Exit |

## Key Concepts

### Anchor
The `Anchor` enum provides 9-point positioning:
```rust
Transform::new(100.0, 200.0).with_anchor(Anchor::BottomCenter)
```

### Offset
Additional pixel offset applied after anchor calculation:
```rust
Transform::new(100.0, 200.0)
    .with_anchor(Anchor::BottomCenter)
    .with_offset(10.0, -5.0)  // Shift 10px right, 5px up
```

## Running

```bash
cd examples/demos/anchor-positioning
cargo run
```
