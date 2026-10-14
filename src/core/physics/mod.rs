//! Rapier physics: bodies, colliders and the simulation world.

mod body;
mod collider;
mod config;
pub(crate) mod convert;
pub(crate) mod debug;
mod events;
mod joint;
mod query;
pub(crate) mod shape;
pub(crate) mod sync;
#[cfg(test)]
pub(crate) mod test_util;
mod triggers;
mod world;

pub use body::{AngularVelocity, BodyKind, RigidBody};
pub use collider::{Collider, ColliderShape};
pub use config::PhysicsConfig;
pub use events::{
  CollisionStarted, CollisionStopped, Contact, ContactForce, JointBroken, TriggerEntered,
  TriggerExited,
};
pub use joint::{Joint, JointAnchor, JointKind, JointMotor};
pub use query::{Hit, Overlap, PointHit, QueryFilter, Touch};
pub use triggers::TriggerContents;
pub use world::Physics;
