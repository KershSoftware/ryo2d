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
mod floor;
pub(crate) mod knockback;
mod lerp;
mod material;
mod property_tween;
mod renderable;
mod sprite_effects;
mod squash_stretch;
mod tween;
mod y_sort;

pub use glam::{IVec2, Mat4, UVec2, Vec2, Vec3, Vec4};

pub use components::{Velocity, ZLayer};
pub use floor::Floor;
pub use kernel::*;
pub use knockback::Knockback;
pub use lerp::Lerp;
pub use material::{Material, ParamValue, Shader, ShaderParams, SpriteShader};
pub(crate) use material::{no_shaders, shaders_of};
pub use pooling::{Pool, PoolConfig, PoolExhaustedPolicy, PoolStats};
pub use property_tween::PropertyTween;
pub use renderable::{BlobShadow, ContactShadow, DropShadow, RenderStyle, Renderable};
pub use sprite_effects::SpriteEffect;
pub use squash_stretch::{SquashAxis, SquashStretch};
pub(crate) use transform::ParentPlaced;
pub use transform::{Anchor, GlobalTransform, Interpolated, PreviousGlobalTransform, Transform};
pub use tween::{Ease, Tween, TweenLoop, TweenSequence};
pub use world_ext::WorldExt;
pub use y_sort::YSort;

/// The order the engine keeps entities in wherever the order shows (a draw
/// order's ties, physics' registration): by index. That is spawn order only
/// for entities spawned fresh: bevy hands a despawned entity's index to a
/// later spawn, which then sorts by that lower index. Never a query's order, which follows the
/// archetypes' layout and changes whenever an archetype is made (every
/// resource is an entity with an archetype of its own), nor a hash set's;
/// and not `Entity`'s own `Ord`, which puts higher indices first. The
/// entity itself breaks the tie between a live entity and a dead one that
/// had its index.
pub(crate) fn entity_order(entity: &Entity) -> (u32, Entity) {
  (entity.index_u32(), *entity)
}

#[cfg(test)]
pub(crate) mod test_util {
  use super::{Component, Resource, World};

  /// Puts an entity in another archetype.
  #[derive(Component)]
  pub(crate) struct Other;

  /// A resource nothing reads.
  #[derive(Resource)]
  struct Unrelated<const N: usize>;

  /// The most resources `insert_unrelated` inserts.
  pub(crate) const MAX_UNRELATED: usize = 8;

  /// Insert `k` (at most `MAX_UNRELATED`) resources nothing reads. Each is an
  /// entity with an archetype of its own, so the entities and archetypes made
  /// after them are numbered `k` higher, and a query visits those archetypes
  /// in another order (bevy_ecs lists them from a hash map).
  pub(crate) fn insert_unrelated(world: &mut World, k: usize) {
    let inserts: [fn(&mut World); MAX_UNRELATED] = [
      |w| w.insert_resource(Unrelated::<0>),
      |w| w.insert_resource(Unrelated::<1>),
      |w| w.insert_resource(Unrelated::<2>),
      |w| w.insert_resource(Unrelated::<3>),
      |w| w.insert_resource(Unrelated::<4>),
      |w| w.insert_resource(Unrelated::<5>),
      |w| w.insert_resource(Unrelated::<6>),
      |w| w.insert_resource(Unrelated::<7>),
    ];
    for insert in &inserts[..k] {
      insert(world);
    }
  }
}
