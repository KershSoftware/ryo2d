//! Physics and collision systems.
//!
//! This module provides:
//! - [`RigidBody`] / [`PhysicsConfig`] - Physics component definitions
//! - [`Collider`] / [`ColliderShape`] - Collision shapes (AABB, Circle)
//! - [`SpatialHash`] / [`PhysicsWorld`] - Broad-phase collision detection
//! - [`RapierPhysicsWorld`] - Rapier2D integration for accurate simulation

pub mod collision;
pub mod components;
pub mod rapier;
pub mod spatial;

pub use collision::{check_collision, Collider, ColliderShape, Collision, AABB, Circle};
pub use components::{PhysicsConfig, RigidBody};
pub use rapier::{CollisionEventType, PhysicsCollisionEvent, RapierPhysicsWorld};
pub use spatial::{CollisionPair, CollisionResult, PhysicsWorld, SpatialHash};
