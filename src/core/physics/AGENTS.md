# AGENTS.md - Physics Module

## Coordinate System

Rapier uses meters, Ryo2D uses pixels:

- **100 pixels = 1 meter** (`PIXELS_PER_METER`, a private const in `rapier.rs`)
- Conversion happens automatically in `RapierPhysicsWorld`
- Gravity is specified in pixels/s² via `PhysicsConfig` (default `(0.0, 980.0)`, +Y = down)

## Notes for AI Agents

1. **Two collision systems**: `SpatialHash`/`PhysicsWorld` (`spatial.rs`) for simple overlap checks, `RapierPhysicsWorld` for full simulation.
2. **Rapier handles physics response**: bodies push each other, and gravity is applied automatically.
3. **Events are one-step**: `step()` clears the collision event list first. Read or drain events each frame or they're lost.
4. **Registration is automatic** (`systems/physics.rs`): an entity enters Rapier only when it has `RigidBody` + `Transform` + (`Collider` or `GroupedCollider`). A `Collider` without a `RigidBody` is never simulated, and there is no static body type. `Inactive` and despawned entities are unregistered, and a world clear (generation change) resets the physics world because hecs reuses entity IDs.
5. **GroupedCollider**: for composite tiles, use `GroupedCollider` from `graphics/tile.rs`.
