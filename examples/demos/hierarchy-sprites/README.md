# Hierarchy Sprites Demo

A demo showcasing the parent-child entity hierarchy system with the unified Renderable component in Ryo2D.

## What It Shows

- **WorldTransform propagation**: Child positions are relative to parents
- **Nested hierarchies**: Sun → Planet → Moon → Satellite
- **Rotation inheritance**: When a parent rotates, all children orbit around it
- **Unified Renderable**: Using the new `Renderable` component instead of `SpriteRenderer`

## Controls

- **Space**: Toggle automatic rotation
- **Escape**: Exit

## How It Works

1. The Sun is positioned at the center of the screen with its own `Transform`
2. The Planet is spawned as a child of the Sun, with a local offset of 120 pixels
3. The Moon is a child of the Planet, offset 40 pixels
4. The Satellite is a child of the Moon, offset 15 pixels

When the Sun rotates, the Planet orbits around it because the Planet's position is relative to the Sun's transform. This cascades down - the Moon orbits the Planet, and the Satellite orbits the Moon.

Each entity also has its own rotation, creating the classic "orbit within orbit" effect.

## Running

From this directory:

```bash
cargo run
```

Or from the workspace root:

```bash
cd examples/games/hierarchy-demo
cargo run
```
