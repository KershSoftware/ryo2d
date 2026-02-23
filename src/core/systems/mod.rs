//! Game systems extracted from the engine update loop.
//!
//! Each system handles a specific concern (animation, tweens, knockback, etc.)
//! and can be updated independently. This makes the code more modular and testable.

mod animation;
mod interpolation;
mod knockback;
mod physics;
mod skeleton;
mod squash_stretch;
mod trail;
mod tween;

pub use animation::AnimationSystem;
pub use interpolation::InterpolationSystem;
pub use knockback::KnockbackSystem;
pub use physics::PhysicsSystem;
pub use skeleton::SkeletonSystem;
pub use squash_stretch::SquashStretchSystem;
pub use trail::TrailSystem;
pub use tween::TweenSystem;
