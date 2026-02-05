# Leap King

![Leap King Demo](../../../docs/assets/leap.gif)

A multiplayer platformer with charge-and-release leap mechanics.

## Features Demonstrated

- **Multiplayer Networking** - Client-server architecture with listen server support
- **Client-Side Prediction** - Smooth movement despite network latency
- **Procedural Generation** - Infinite vertical platforms from shared seed
- **2D Lighting** - Point lights on players and platform lamps
- **Particle System** - Rain emitter and visual effects
- **Camera Follow** - Smooth vertical camera tracking

## Controls

Slingshot-style aiming: pull in the opposite direction you want to leap.

- `Hold Space` - Charge leap power
- `A` / `←` - Pull left to leap **right**
- `D` / `→` - Pull right to leap **left**
- `W` / `↑` - Pull up for lower arc
- `S` / `↓` - Pull down for higher arc
- `Release Space` - Leap
- `Escape` - Quit

## Run

```bash
ryo2d dev          # Development mode
ryo2d build && ryo2d run  # Production build
```
