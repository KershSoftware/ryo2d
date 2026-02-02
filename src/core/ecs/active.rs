//! Active/Inactive marker components for object pooling
//!
//! These marker components allow entities to be deactivated without despawning.
//! Inactive entities are skipped by rendering and physics systems but remain
//! in the ECS world for fast reuse by object pools.

/// Marker component indicating an entity is active and should be processed.
///
/// Entities with this component are rendered, updated by physics, and included
/// in gameplay systems. This is the default state for most entities.
///
/// # Example
///
/// ```rust,ignore
/// // Spawn an active bullet
/// world.spawn((
///     Transform::new(x, y),
///     Renderable::sprite(bullet_sprite),
///     Collider::circle(4.0),
///     Active,
///     Bullet { velocity_x: vx, velocity_y: vy },
/// ));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Active;

/// Marker component indicating an entity is pooled but inactive.
///
/// Entities with this component are:
/// - **Skipped by rendering** - not drawn to screen
/// - **Skipped by physics** - not registered with Rapier
/// - **Skipped by animation systems** - animators don't update
///
/// This allows object pools to keep entities in the ECS world without them
/// affecting gameplay. When an entity is acquired from a pool, `Inactive` is
/// removed and `Active` is added.
///
/// # Example
///
/// ```rust,ignore
/// // Return bullet to pool (deactivate it)
/// world.remove_one::<Active>(entity);
/// world.insert_one(entity, Inactive);
/// // Entity is now invisible and won't collide, but still exists
///
/// // Later, reactivate it
/// world.remove_one::<Inactive>(entity);
/// world.insert_one(entity, Active);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inactive;
