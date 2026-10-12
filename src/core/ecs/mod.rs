//! Ryo2D's ECS: the kernel types plus the engine's own components.
//!
//! Everything a game needs is exported here (and from the prelude):
//! `World`, `Entity`, `Query`, `Commands`, the derives, `Transform` and
//! `GlobalTransform`, and the engine's components. The engine's own systems
//! stay in their modules (`propagate::propagate_transforms`,
//! `interpolate::render_global`, ...).

pub mod interpolate;
pub mod kernel;
pub mod pooling;
pub mod propagate;
pub mod transform;
pub mod world_ext;

mod components;
pub(crate) mod knockback;
mod lerp;
mod material;
mod property_tween;
mod renderable;
mod sprite_effects;
mod squash_stretch;
mod tween;
mod y_sort;

pub use glam::{Mat4, Vec2, Vec3, Vec4};

pub use components::{RenderLayer, Velocity};
pub use kernel::*;
pub use knockback::Knockback;
pub use lerp::Lerp;
pub use material::{Material, ParamValue, ShaderHandle, ShaderParams, SpriteShader};
pub use pooling::{Pool, PoolConfig, PoolExhaustedPolicy, PoolStats};
pub use property_tween::PropertyTween;
pub use renderable::{BlobShadow, ContactShadow, DropShadow, RenderStyle, Renderable};
pub use sprite_effects::SpriteEffect;
pub use squash_stretch::{SquashAxis, SquashStretch};
pub use transform::{Anchor, GlobalTransform, Interpolated, PreviousGlobalTransform, Transform};
pub use tween::{Ease, Tween, TweenLoop, TweenSequence};
pub use world_ext::WorldExt;
pub use y_sort::YSort;
