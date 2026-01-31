//! Simple Entity-Component-System (ECS) module
//!
//! This module provides a thin wrapper around `hecs` with a simplified API.
//! Key features:
//!
//! - **Generational entity IDs** - Prevents use-after-free bugs with despawned entities
//! - **Archetype-based storage** - Cache-friendly iteration
//! - **Thread-safe** - Components must be `Send + Sync`
//! - **Hierarchy support** - Parent-child relationships with transform propagation
//! - **Per-sprite shaders** - Material components for GPU effects

mod component;
mod components;
mod hierarchy;
mod material;
mod renderable;
mod sprite_effects;
mod world;

pub use component::Component;
pub use components::{Mat4, RenderLayer, Transform, Vec2, Vec3, Vec4, Velocity};
pub use hecs::Entity;
pub use hierarchy::{Children, Parent, WorldTransform};
pub use material::{Material, ShaderHandle, ShaderParams, SpriteShader};
pub use renderable::{RenderMode, RenderStyle, Renderable};
pub use sprite_effects::SpriteEffect;
pub use world::World;
