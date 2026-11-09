//! Particles
//!
//! A CPU simulation the engine runs in its fixed step and draws through the
//! 2D batch, one run per emitter: a game describes a kind of particle once,
//! as a [`ParticleEffect`] asset, and places it with [`ParticleEmitter`]
//! entities. No call from the game steps or draws them.
//!
//! # Overview
//!
//! - Each fixed step, every enabled emitter (in entity order, each from its
//!   own random stream) spawns its effect's particles at its
//!   `GlobalTransform`, moves them under gravity, drag, the effect's curves
//!   and the [`ParticleAttractor`]s around it, collides them with the
//!   world's colliders when the effect has a [`CollisionConfig`], and
//!   removes the dead. An emitter made `unscaled` steps on real time
//!   instead, through pauses and held scenes.
//! - Each frame, every emitter with live particles that a camera sees joins
//!   the draw list as one run of sprites, at the emitter's z (else its
//!   effect's), each particle blended between its last two steps.
//! - [`ParticlesView`] (`ctx.particles()`) and the [`Particles`] system
//!   parameter play an effect once where something happened (`play_at`),
//!   and read the live count against the engine's budget
//!   ([`DEFAULT_PARTICLE_BUDGET`] unless `EngineBuilder::particle_budget`
//!   sets another).
//!
//! # Types
//!
//! | Type | Description |
//! |------|-------------|
//! | [`ParticleEffect`] (asset) | How a kind of particle is born, moves, looks and ends; presets such as [`ParticleEffect::fire`] |
//! | [`ParticleEmitter`] (component) | Plays an effect at its entity: `play`, `stop`, `burst`, `once`, `despawn_when_finished` |
//! | [`ParticleAttractor`] (component) | A force field that pulls, swirls or pushes particles |
//! | [`CollisionConfig`] | How an effect's particles meet colliders, and whether they report [`ParticleCollision`] |
//! | [`EmitterFinished`] (message) | An emitter stopped and its last particle died |
//!
//! # Quick start
//!
//! ```
//! use ryo2d::prelude::*;
//!
//! fn enter(ctx: &mut SceneContext<'_>) -> Handle<ParticleEffect> {
//!   let spark = ctx
//!     .assets()
//!     .add(Image::solid_circle(16, 255, 255, 255, 255));
//!   // A fire that burns where its entity stands, until it is despawned
//!   let fire = ctx.assets().add(ParticleEffect::fire(spark.clone()));
//!   ctx.world.spawn((
//!     Transform::from_xy(100.0, 100.0),
//!     ParticleEmitter::new(fire),
//!   ));
//!   // A ring of cyan particles drifting outward
//!   let ring = ctx.assets().add(
//!     ParticleEffect::new(spark.clone())
//!       .with_rate(30.0)
//!       .with_shape(EmitShape::Circle { radius: 20.0, arc_deg: 360.0, thickness: 0.0 })
//!       .outward()
//!       .with_lifetime((1.0, 2.0))
//!       .with_color(Color::CYAN),
//!   );
//!   ctx.world.spawn((Transform::from_xy(300.0, 100.0), ParticleEmitter::new(ring)));
//!   // An attractor pulling particles towards it
//!   ctx.world.spawn((
//!     Transform::from_xy(200.0, 200.0),
//!     ParticleAttractor::point(200.0, 100.0),
//!   ));
//!   // Kept for bursts: `ctx.particles().play_at(&boom, at)` plays it once
//!   ctx.assets().add(ParticleEffect::explosion(spark, 50))
//! }
//! ```

mod attractor;
mod collision;
mod contact;
mod draw;
mod effect;
mod emitter;
mod forces;
mod noise;
mod pool;
mod presets;
mod rng;
mod scope;
mod space;
mod step;
mod store;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
mod value;
mod view;

pub use attractor::{AttractorType, ParticleAttractor};
pub use collision::{
  CollisionConfig, CollisionResponse, MIN_BOUNCE_EVENT_SPEED, ParticleCollision,
};
pub(crate) use draw::ParticleDrawer;
pub use effect::{
  Align, Burst, DrawOrder, EmitShape, Flipbook, FrameMode, NoiseForce, ParticleEffect, Space,
};
pub use emitter::{EmitterFinished, ParticleEmitter};
pub(crate) use scope::{hold_scenes, in_scene, release_scene};
pub(crate) use step::{step_particles, step_unscaled_particles};
pub use store::DEFAULT_PARTICLE_BUDGET;
pub(crate) use store::{PARTICLE_HEADLESS_SEED, ParticleStore};
pub use value::{ColorRange, Curve, CurveKey, Gradient, GradientKey, ValueRange};
pub use view::{Particles, ParticlesView};
