//! The particles as a game reaches them: `ctx.particles()`
//! ([`ParticlesView`]) in scenes and the [`Particles`] parameter in systems.

use glam::Vec2;

use super::effect::ParticleEffect;
use super::emitter::ParticleEmitter;
use super::store::{DEFAULT_PARTICLE_BUDGET, ParticleStore};
use crate::core::asset::Handle;
use crate::core::ecs::kernel::{Commands, Res, SystemParam};
use crate::core::ecs::{Entity, Transform, World};

/// The engine's particles seen from a scene: play an effect once where
/// something happened, read the live count against the budget, and clear
/// every particle. A scene gets one from `ctx.particles()`; it borrows the
/// world, so take it, use it and let it go (a system takes [`Particles`]
/// instead, and an exclusive system `ParticlesView::new(world)`).
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn explode(ctx: &mut SceneContext<'_>, sparks: &Handle<ParticleEffect>, at: Vec2) {
///   // One cycle of the effect, then the emitter despawns itself
///   ctx.particles().play_at(sparks, at);
/// }
/// ```
///
/// A world without the engine's particles reads no particles and the
/// default budget, and its emitters never emit.
pub struct ParticlesView<'w> {
  world: &'w mut World,
}

impl<'w> ParticlesView<'w> {
  /// The particles of `world`
  pub fn new(world: &'w mut World) -> Self {
    Self { world }
  }

  /// Plays `effect` once at `position`: an emitter entity that plays one
  /// cycle of it (even of a looping effect) and despawns when its last
  /// particle dies, sending `EmitterFinished` first. Returns the entity
  /// (move it, parent it or despawn it as any other). Spawned from a
  /// scene's code, it belongs to that scene, and is held with it.
  pub fn play_at(&mut self, effect: &Handle<ParticleEffect>, position: Vec2) -> Entity {
    self.world.spawn(one_shot(effect, position)).id()
  }

  /// Live particles over every emitter
  pub fn count(&self) -> u32 {
    self
      .world
      .get_resource::<ParticleStore>()
      .map_or(0, ParticleStore::live)
  }

  /// The most live particles allowed at once (`EngineBuilder::particle_budget`)
  pub fn budget(&self) -> u32 {
    self
      .world
      .get_resource::<ParticleStore>()
      .map_or(DEFAULT_PARTICLE_BUDGET, ParticleStore::budget)
  }

  /// Removes every live particle at once. The emitters carry on: those
  /// that play go on emitting.
  pub fn clear(&mut self) {
    if let Some(mut store) = self.world.get_resource_mut::<ParticleStore>() {
      store.clear_all();
    }
  }
}

/// The engine's particles as a system reaches them: take it as a parameter
/// (`mut particles: Particles`) for what [`ParticlesView`] does in a scene.
/// Its `play_at` spawns through the system's commands, so the emitter
/// exists once they apply (the entity it returns is reserved at once).
///
/// ```
/// use ryo2d::prelude::*;
///
/// #[derive(Resource)]
/// struct Sparks(Handle<ParticleEffect>);
///
/// #[derive(Component)]
/// struct Hit;
///
/// /// Sparks where each hit lands
/// fn sparks(
///   mut commands: Commands,
///   mut particles: Particles,
///   sparks: Res<Sparks>,
///   hits: Query<(Entity, &Transform), With<Hit>>,
/// ) {
///   for (entity, transform) in &hits {
///     particles.play_at(&sparks.0, transform.translation);
///     commands.entity(entity).remove::<Hit>();
///   }
/// }
/// ```
#[derive(SystemParam)]
pub struct Particles<'w, 's> {
  store: Option<Res<'w, ParticleStore>>,
  commands: Commands<'w, 's>,
}

impl Particles<'_, '_> {
  /// Plays `effect` once at `position`, as [`ParticlesView::play_at`]
  /// does, when the system's commands apply
  pub fn play_at(&mut self, effect: &Handle<ParticleEffect>, position: Vec2) -> Entity {
    self.commands.spawn(one_shot(effect, position)).id()
  }

  /// Live particles over every emitter
  pub fn count(&self) -> u32 {
    self.store.as_deref().map_or(0, ParticleStore::live)
  }

  /// The most live particles allowed at once
  pub fn budget(&self) -> u32 {
    self
      .store
      .as_deref()
      .map_or(DEFAULT_PARTICLE_BUDGET, ParticleStore::budget)
  }
}

/// An emitter entity playing one cycle of `effect` at `position`, gone when
/// it finishes
fn one_shot(effect: &Handle<ParticleEffect>, position: Vec2) -> (Transform, ParticleEmitter) {
  (
    Transform::from_xy(position.x, position.y),
    ParticleEmitter::new(effect.clone())
      .once()
      .despawn_when_finished(),
  )
}
