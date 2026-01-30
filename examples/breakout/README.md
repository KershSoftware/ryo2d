# Breakout - Ryo2D Example Game

A complete Breakout/Arkanoid clone demonstrating the major features of the Ryo2D game engine.

## Game Overview

Classic brick-breaking arcade action! Control a paddle to bounce a ball and destroy all the blocks. Complete levels, earn points, and set high scores.

## Ryo2D Features Demonstrated

This example showcases a comprehensive set of engine capabilities:

### Core Systems
- **Scene Management** - Multiple game states (Menu, Gameplay, Pause, Quit Confirmation)
- **ECS Architecture** - Entity-Component-System for game entities (Paddle, Ball, Blocks, Walls)
- **Immediate-Mode UI** - Menus, HUD, and interactive buttons
- **Save System** - Persistent high score storage using workspace configuration

### Physics & Collision
- **Rapier2D Integration** - Rigid body physics with kinematic and dynamic bodies
- **Collision Detection** - Ball bouncing off paddle, walls, and blocks
- **Collision Events** - Game logic triggered by physics interactions

### Content & Data
- **Level Loading** - External level files loaded from disk
- **Audio System** - Background music, sound effects, and runtime volume controls
  - Looping background music during gameplay
  - Sound effects for paddle hits, block breaks, and ball death
  - Interactive volume sliders in pause menu (Master, Music, SFX)
  - Volume settings persist via system.json
- **Asset Management** - Sprite rendering and resource handling

## Controls

- **Menu**: Click "Start", or press Enter/Space to begin
- **Gameplay**: 
  - `A` / `D` or `←` / `→` - Move paddle
  - `Space` or click - Launch ball
  - `Escape` - Pause game
- **Pause Menu**: Click buttons or use keyboard to navigate
  - Adjust audio volumes with interactive sliders
  - Changes are saved automatically to system.json

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
cd examples/breakout
ryo2d run
```

## Project Structure

```
breakout/
├── src/
│   ├── components/    # ECS components (Paddle, Ball, Block, etc.)
│   ├── scenes/        # Game scenes (Menu, Game, Pause, QuitConfirm)
│   ├── levels/        # Level loading utilities
│   ├── save/          # High score persistence
│   └── constants.rs   # Game configuration
├── assets/            # Sprites and audio
├── levels/            # Level data files
├── data/              # Save data (auto-generated)
├── main.rs            # Game entry point
└── Ryo.toml          # Project configuration
```

## Implementation Notes

- **Physics tick rate**: 120 Hz for stable ball physics
- **Render rate**: 144 FPS
- **Level format**: RON (Rusty Object Notation) for easy editing
- **Save location**: Configured via `Ryo.toml`, stored in `data/` directory
