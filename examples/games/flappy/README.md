# Flappy - Ryo2D Example Game

A Flappy clone showcasing scrolling, procedural generation, parallax backgrounds, and physics features of the Ryo2D game engine.

## Game Overview

Guide a bird through an endless series of pipes by tapping to flap. Time your flaps carefully to avoid hitting the pipes or the ground. See how high you can score!

## Ryo2D Features Demonstrated

This example highlights key engine capabilities for side-scrolling and arcade games:

### Asset Loading & Rendering
- **Image Asset Loading** - PNG sprites loaded from disk using `Sprite::load()`
- **Sprite Rendering** - Efficient sprite rendering with the ECS
- **Sprite Scaling** - Dynamic sprite scaling for variable-height pipes
- **Z-ordering** - Layered rendering with depth sorting

### Scrolling & Camera
- **Parallax Scrolling** - Multi-layered backgrounds with varying scroll speeds for depth
- **Custom Scrolling Component** - Reusable `Scrolling` component for seamless looping
- **Camera Management** - Smooth scrolling backgrounds and ground
- **Procedural Generation** - Infinite pipe obstacles with randomized heights

### Entity Component System (ECS)
- **Hierarchical Sprites** - Parent-child entity relationships for composite objects
- **Component-Based Design** - Clean separation of data (Transform, Velocity, Renderable, etc.)
- **Entity Management** - Dynamic spawning and despawning of obstacles
- **Query System** - Efficient iteration over entities with specific components

### Physics & Gameplay
- **Custom Physics** - Gravity and velocity system for bird movement
- **Collision Detection** - Bird vs pipes and ground boundaries
- **Flap Mechanics** - Impulse-based jump with gravity simulation

### Core Systems
- **Scene Management** - Menu → Gameplay → Game Over flow
- **Save System** - High score persistence via workspace configuration
- **Immediate-Mode UI** - Score display and menu screens
- **Asset loading**: `Sprite::load()` reads PNG files from `assets/` directory
- **Dynamic scaling**: Pipes are scaled vertically to match random heights

## Controls

- **Menu**: `Space` or `Enter` to start
- **Gameplay**: `Space` to flap
- **Game Over**: Click buttons to restart or return to menu

## Running the Game

### Using Cargo (from this directory)

```bash
cargo run
```

### Using the Ryo2D CLI

```bash
# From the ryo2d project root
./run flappy

# From the example game root
ryo2d dev # development runner
# or
ryo2d build && ryo2d run # production runner
```
