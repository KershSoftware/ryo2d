//! Simple Entity-Component-System (ECS) module
//!
//! This module provides a thin wrapper around `hecs` with a simplified API.
//! Key features:
//!
//! - **Generational entity IDs** - Prevents use-after-free bugs with despawned entities
//! - **Archetype-based storage** - Cache-friendly iteration
//! - **Thread-safe** - Components must be `Send + Sync`

mod component;
mod components;
mod world;

pub use component::Component;
pub use components::{Mat4, RenderLayer, SpriteRenderer, Transform, Vec2, Vec3, Vec4, Velocity};
pub use hecs::Entity;
pub use world::World;
