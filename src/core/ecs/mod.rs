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
//! - **Object pooling** - Pre-allocated entity pools for high-frequency spawning

mod active;
mod component;
mod components;
mod hierarchy;
mod knockback;
mod material;
mod pool;
mod renderable;
mod sprite_effects;
mod squash_stretch;
mod tween;
mod world;
mod y_sort;

pub use active::{Active, Inactive};
pub use component::Component;
pub use components::{Mat4, PreviousTransform, RenderLayer, Transform, Vec2, Vec3, Vec4, Velocity};
pub use hecs::Entity;
pub use hierarchy::{Children, Parent, WorldTransform};
pub use knockback::Knockback;
pub use material::{Material, ShaderHandle, ShaderParams, SpriteShader};
pub use pool::{Pool, PoolConfig, PoolExhaustedPolicy, PoolStats};
pub use renderable::{DropShadow, RenderMode, RenderStyle, Renderable};
pub use sprite_effects::SpriteEffect;
pub use squash_stretch::{SquashAxis, SquashStretch};
pub use tween::{Ease, Tween, TweenLoop, TweenSequence};
pub use world::World;
pub use y_sort::YSort;
