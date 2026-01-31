# Particles Demo

High-performance particle physics demonstration with 1000+ dynamic sprites.

## Features

- **Three Rendering Modes:**
  - Created Sprites: Procedurally generated colored circles
  - Imported Sprites: Image-based particles with glow effects
  - Hierarchical Sprites: Parent-child particle clusters

- **Physics Simulation:**
  - Gravity-driven particle fall
  - Collision detection between particles
  - Wall collision with bounce
  - Realistic restitution and friction

- **Performance:**
  - Handles 2000+ particles at 60 FPS
  - Efficient collision detection via Rapier2D
  - Optimized sprite rendering

## Controls

- **1/2/3** - Switch between rendering modes
- **Space** - Spawn 100 more particles
- **R** - Reset scene
- **Escape** - Quit

## Running

```bash
cargo run --release
```

Release mode is recommended for smooth performance with high particle counts.

## Implementation Notes

This demo showcases:
- Procedural sprite generation with gradients
- Physics integration (RigidBody, Collider components)
- Entity-component-system patterns
- Parent-child entity hierarchies
- Immediate-mode UI rendering

The hierarchical mode demonstrates Ryo2D's parent-child transform system where child particles orbit their parents while the parent moves under physics simulation.
