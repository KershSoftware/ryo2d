//! Game systems extracted from the engine update loop.
//!
//! Each system handles a specific concern (animation, tweens, knockback, etc.)
//! and can be updated independently. This makes the code more modular and testable.

mod animation;
mod tween;

pub use animation::AnimationSystem;
pub use tween::TweenSystem;
