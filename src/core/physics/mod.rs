//! Rapier2D physics: bodies, colliders and the simulation world.

pub mod collision;
pub mod components;
pub mod rapier;

pub use collision::{Collider, ColliderShape};
pub use components::{PhysicsConfig, RigidBody};
pub use rapier::{CollisionEventType, PhysicsCollisionEvent, RapierPhysicsWorld};
