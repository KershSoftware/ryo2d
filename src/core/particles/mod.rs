//! GPU Particle System
//!
//! A full-featured particle system with CPU simulation and GPU rendering.
//!
//! # Overview
//!
//! This particle system uses **CPU simulation + GPU rendering**:
//! - Particles are simulated on the CPU (physics, attractors, collisions)
//! - Rendering uses the existing `SpriteGpuRenderer` with instanced drawing
//! - Supports 10k+ particles at 60fps
//!
//! A scene owns a [`ParticleSystem`]: it calls `update(ctx.world, dt)` from
//! `Scene::update` and `collect_for_render` from `Scene::render`. The engine
//! does not drive particles itself.
//!
//! # Types
//!
//! | Type | Description |
//! |------|-------------|
//! | [`ParticleSystem`] | Simulates every emitter's particles and batches them for drawing |
//! | [`Particle`] | Individual particle data (velocity, lifetime, color) |
//! | [`ParticleEmitter`] (component) | Spawns particles with configurable shapes and rates |
//! | [`ParticleAttractor`] (component) | Force fields that affect particle motion |
//!
//! # Quick Start
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // The scene owns the system
//! struct Level { particles: ParticleSystem }
//!
//! fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
//!     self.particles.update(ctx.world, dt);
//!     SceneAction::Continue
//! }
//!
//! fn render(&mut self, gpu: &mut GpuRenderContext) {
//!     self.particles.collect_for_render(gpu.world, gpu.batch);
//! }
//!
//! // Spawn a fire effect
//! let fire_sprite = ctx.assets().load::<Image>("particle.png");
//! ctx.world.spawn((
//!     Transform::from_xy(100.0, 100.0),
//!     ParticleEmitter::fire(fire_sprite),
//! ));
//!
//! // One-shot explosion
//! ctx.world.spawn((
//!     Transform::from_xy(x, y),
//!     ParticleEmitter::explosion(spark_sprite, 50).one_shot(),
//! ));
//!
//! // Custom emitter with attractor
//! ctx.world.spawn((
//!     Transform::from_xy(x, y),
//!     ParticleEmitter::new(sprite)
//!         .with_mode(EmissionMode::Continuous { rate: 30.0 })
//!         .with_shape(EmissionShape::Circle { radius: 20.0, hollow: true })
//!         .with_lifetime(1.0, 2.0)
//!         .with_color(Color::CYAN, Color::TRANSPARENT),
//! ));
//!
//! // Attractor that pulls particles
//! ctx.world.spawn((
//!     Transform::from_xy(mouse_x, mouse_y),
//!     ParticleAttractor::point(200.0, 100.0),
//! ));
//! ```
//!
//! # Built-in Presets
//!
//! | Preset | Description |
//! |--------|-------------|
//! | `ParticleEmitter::fire(sprite)` | Upward orange-yellow flames |
//! | `ParticleEmitter::smoke(sprite)` | Slow expanding gray particles |
//! | `ParticleEmitter::explosion(sprite, count)` | Radial burst with trails |
//! | `ParticleEmitter::magic(sprite)` | Glowing wandering sparkles |
//! | `ParticleEmitter::rain(sprite)` | Downward streaks |
//! | `ParticleEmitter::sparks(sprite)` | Bouncing bright particles |
//! | `ParticleEmitter::snow(sprite)` | Gentle floating descent |
//! | `ParticleEmitter::confetti(sprite)` | Multi-colored tumbling |

mod attractor;
mod collision;
mod contact;
mod legacy;
mod noise;
mod rng;
mod value;

pub use attractor::{AttractorType, ParticleAttractor};
pub use collision::{
  CollisionConfig, CollisionResponse, MIN_BOUNCE_EVENT_SPEED, ParticleCollision,
};
pub use legacy::{EmissionMode, EmissionShape, Particle, ParticleEmitter, ParticleSystem};
pub use value::{ColorRange, Curve, CurveKey, Gradient, GradientKey, ValueRange};
