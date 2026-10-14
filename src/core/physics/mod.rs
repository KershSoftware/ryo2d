//! Rapier physics: bodies, colliders and the simulation world.

mod body;
mod collider;
mod config;
pub(crate) mod convert;
pub(crate) mod debug;
mod query;
pub(crate) mod shape;
pub(crate) mod sync;
#[cfg(test)]
pub(crate) mod test_util;
mod world;

pub use body::{AngularVelocity, BodyKind, RigidBody};
pub use collider::{Collider, ColliderShape};
pub use config::PhysicsConfig;
pub use world::Physics;
