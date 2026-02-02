# AGENTS.md - Physics Module

## Overview

The `physics/` module provides collision detection and physics simulation via Rapier2D integration.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports and re-exports |
| `components.rs` | `RigidBody`, `PhysicsConfig` - ECS components |
| `collision.rs` | `Collider`, `AABB`, `Circle` - collision shapes |
| `spatial.rs` | `SpatialHash`, `PhysicsWorld` - broad-phase detection |
| `rapier.rs` | `RapierPhysicsWorld` - Rapier2D integration |

## Key Types

### RigidBody

Physics body component:

```rust
// Dynamic body (affected by forces)
RigidBody::dynamic()

// Kinematic body (controlled by code, affects others)
RigidBody::kinematic()

// Static body (immovable)
RigidBody::stationary()
```

### Collider

Collision shape component:

```rust
// Box collider
Collider::box_collider(width, height)

// Circle collider
Collider::circle(radius)

// With offset from transform
Collider::box_collider(32.0, 32.0).with_offset(0.0, 16.0)
```

### Collision Shapes

Low-level collision primitives:

```rust
// AABB (Axis-Aligned Bounding Box)
let aabb = AABB::new(x, y, width, height);
aabb.intersects(&other_aabb);

// Circle
let circle = Circle::new(x, y, radius);
circle.intersects_aabb(&aabb);

// Generic collision check
if check_collision(&collider_a, pos_a, &collider_b, pos_b) {
    // Handle collision
}
```

### SpatialHash

Efficient broad-phase collision detection:

```rust
let mut spatial = SpatialHash::new(64.0); // 64px cells

// Insert entities
spatial.insert(entity, x, y, width, height);

// Query area
let nearby = spatial.query(x, y, width, height);
```

### RapierPhysicsWorld

Full physics simulation with Rapier2D:

```rust
// Created by engine, accessed via context
let physics = &mut ctx.physics;

// Step simulation
physics.step(dt, &mut world);

// Get collision events
for event in physics.drain_collision_events() {
    match event.event_type {
        CollisionEventType::Started => { /* collision began */ }
        CollisionEventType::Stopped => { /* collision ended */ }
    }
}

// Register entities
physics.register_entity(entity, &transform, &collider, &rigid_body);
```

## Import Patterns

From within this module (sibling files):
```rust
use super::collision::{Collider, ColliderShape};
use super::components::RigidBody;
```

From other core modules:
```rust
use crate::core::physics::{RigidBody, Collider, RapierPhysicsWorld};
use crate::core::physics::{check_collision, AABB, Circle};
```

From game code via prelude:
```rust
use ryo2d::prelude::*;
// RigidBody, Collider, PhysicsCollisionEvent, etc. available
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      ECS Components                          │
│  RigidBody, Collider, Transform, Velocity                   │
└─────────────────────────────────────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                   RapierPhysicsWorld                         │
│  - Syncs ECS ↔ Rapier bodies                                │
│  - Steps simulation                                          │
│  - Collects collision events                                 │
└─────────────────────────────────────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                     Rapier2D (external)                      │
│  - Rigid body simulation                                     │
│  - Collision detection                                       │
│  - Contact resolution                                        │
└─────────────────────────────────────────────────────────────┘
```

## Coordinate System

Rapier uses meters, Ryo2D uses pixels:

- **100 pixels = 1 meter** (configurable)
- Conversion happens automatically in `RapierPhysicsWorld`
- Gravity default: 9.8 m/s² (980 pixels/s²)

## Notes for AI Agents

1. **Two collision systems** - SpatialHash for simple checks, Rapier for full simulation
2. **Rapier handles physics response** - Bodies push each other, gravity applied automatically
3. **Events are one-frame** - Drain collision events each frame or they're lost
4. **Register before simulate** - Entities must be registered with physics world
5. **GroupedCollider** - For composite tiles, use `GroupedCollider` from `graphics/tile.rs`
