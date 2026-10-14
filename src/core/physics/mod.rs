//! Rapier physics: bodies, colliders and the simulation world.

pub mod collision;
pub mod components;
mod config;
pub(crate) mod convert;
pub(crate) mod sync;
mod world;

pub use collision::{Collider, ColliderShape};
pub use components::RigidBody;
pub use config::PhysicsConfig;
pub use world::{CollisionEventType, Physics, PhysicsCollisionEvent};
