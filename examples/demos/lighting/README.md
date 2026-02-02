# Lighting Demo

A demonstration of the Ryo2D 2D lighting system featuring a pendulum lamp swinging in a dark room.

**⚠️ NOTE:** The lighting system is currently not integrated with the rendering pipeline. This demo shows the component API and physics simulation, but lighting effects are not yet visible. The lighting renderer implementation exists in `src/core/lighting/renderer.rs` but needs to be wired into the main render loop in `src/core/engine.rs`.

## Features

- **Dynamic Point Light** - A warm torch light attached to the swinging lamp
- **Pendulum Physics** - Realistic pendulum motion with gravity and damping
- **Light Occluders** - Walls, floor, and objects that cast shadows
- **Multiple Shapes** - Various colored boxes on the floor and floating in the air to demonstrate lighting effects

## Controls

- **SPACE** - Reset the pendulum to its initial position
- **ESC** - Quit the demo

## Building and Running

### From this directory

```bash
# Run in debug mode (faster compilation, slower runtime)
cargo run

# Or use the Ryo2D CLI
ryo2d dev
```

### From the Ryo2D repository root

```bash
# Run directly with the root run script
./run lighting-demo
```

### Release

```bash
# Build optimized release
ryo2d build

# Run the release build
ryo2d run
```

## What It Demonstrates

1. **Ambient Lighting** - The room has a very dark ambient light to make dynamic lights stand out
2. **Point Lights** - The lamp emits a warm orange torch light that follows the pendulum motion
3. **Shadow Casting** - Objects block light and cast realistic shadows
4. **Light Falloff** - Light intensity decreases with distance using physically-based falloff
5. **Pendulum Physics** - Simple angular physics simulation with gravity and damping

## Implementation Details

The demo uses:

- `AmbientLight` resource for global base illumination
- `Light2D::torch()` for the swinging lamp light
- `LightOccluder2D::rect()` for walls and object shadows
- Custom pendulum physics simulation for realistic motion
- Transform updates to sync lamp and light positions

## Code Highlights

```rust
// Set dark ambient light
ctx.world.insert_resource(
  AmbientLight::new(Color::new(10, 10, 15, 255), 0.15)
);

// Create the lamp light
ctx.world.spawn((
  Transform::at(lamp_x, lamp_y),
  Light2D::torch(250.0).with_intensity(1.2),
));

// Add occluders for shadow casting
ctx.world.spawn((
  Transform::at(x, y),
  LightOccluder2D::rect(width, height),
));
```

## Project Structure

```
lighting-demo/
├── main.rs          # Game entry point with pendulum physics
├── Cargo.toml       # Rust package configuration
├── Ryo.toml         # Ryo2D project configuration
├── assets/          # Game assets (auto-generated)
└── data/            # Save data (auto-generated)
```

## License

Same as Ryo2D engine.
