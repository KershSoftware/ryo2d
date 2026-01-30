# Flappy Bird - Ryo2D Example Game

A Flappy Bird clone showcasing scrolling, procedural generation, and physics features of the Ryo2D game engine.

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
- **Parallax Scrolling** - Custom `Scrolling` component for layered movement
- **Camera Management** - Smooth scrolling backgrounds and ground
- **Procedural Generation** - Infinite pipe obstacles with randomized heights

### Physics & Gameplay
- **Custom Physics** - Gravity and velocity system for bird movement
- **Collision Detection** - Bird vs pipes and ground boundaries
- **Entity Management** - Dynamic spawning and despawning of obstacles

### Core Systems
- **Scene Management** - Menu → Gameplay → Game Over flow
- **ECS Architecture** - Clean component-based entity design
- **Save System** - High score persistence via workspace configuration
- **Immediate-Mode UI** - Score display and menu screens

### Visual Features
- **Asset Loading** - Images loaded from `assets/` directory
- **Sprite Rendering** - PNG sprites for bird, pipes, and ground
- **Dynamic Scaling** - Pipes scaled to fit variable heights
- **Z-ordering** - Layered rendering with depth sorting

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
# From the project root
ryo2d run

# Or from anywhere in the project
cd examples/flappy
ryo2d run
```

## Project Structure

```
flappy/
├── src/
│   ├── components/       # ECS components
│   │   ├── bird.rs       # Player character with physics
│   │   ├── pipe.rs       # Obstacle pipes
│   │   ├── ground.rs     # Scrolling floor
│   │   └── scrolling.rs  # Parallax scroll component
│   ├── scenes/           # Game screens
│   │   ├── menu.rs       # Start menu
│   │   ├── game.rs       # Main gameplay
│   │   └── game_over.rs  # Results screen
│   ├── constants.rs      # Game configuration
│   ├── save.rs           # High score persistence
│   └── lib.rs            # Module exports
├── assets/               # Sprites (ready for images)
├── data/                 # Save data (auto-generated)
├── main.rs               # Game entry point
└── Ryo.toml             # Project configuration
```

## Implementation Notes

- **Physics tick rate**: 120 Hz for smooth bird movement
- **Render rate**: 60 FPS
- **Pipe spawning**: Procedurally generated with random gap positions
- **Scrolling system**: Reusable component for parallax effects
- **Asset loading**: `Sprite::load()` reads PNG files from `assets/` directory
- **Dynamic scaling**: Pipes are scaled vertically to match random heights

## Asset Generation

The game includes simple colored sprite assets. To regenerate them:

```bash
cargo run --bin generate_assets --features generate-assets
```

This creates three PNG files:
- `assets/bird.png` - 34x34 yellow square
- `assets/pipe.png` - 52x200 green rectangle
- `assets/ground.png` - 400x112 brown rectangle

You can replace these with custom artwork of the same dimensions.
