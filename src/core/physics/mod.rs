//! Rapier physics: bodies, colliders and the simulation world.

mod body;
pub mod collision;
mod config;
pub(crate) mod convert;
pub(crate) mod debug;
mod query;
pub(crate) mod sync;
#[cfg(test)]
pub(crate) mod test_util;
mod world;

pub use body::{AngularVelocity, BodyKind, RigidBody};
pub use collision::{Collider, ColliderShape};
pub use config::PhysicsConfig;
pub use world::Physics;
