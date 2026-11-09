//! Drawing the engine's particles: each visible emitter's pool joins the
//! frame's draw list as one run.

use glam::Vec2;

use super::emitter::ParticleEmitter;
use super::pool::{EmitterPool, Pool};
use super::space::LocalFrame;
use super::step::Compiled;
use super::store::ParticleStore;
use crate::core::asset::Assets;
use crate::core::ecs::kernel::QueryState;
use crate::core::ecs::{
  DrawLift, Entity, GlobalTransform, PreviousGlobalTransform, World, entity_order,
};
use crate::core::graphics::{Image, RenderLayers, SpriteGpuRenderer, SpriteInstance};
use crate::core::render::{CameraDraw, DrawList, DrawRun, SpriteShading};

/// The emitters drawn: enabled ones (the default filter leaves `Disabled`
/// out, so a disabled emitter's particles are hidden), with their layers,
/// and where they stand for a `Local` effect's particles
type DrawnEmitter = (
  Entity,
  &'static ParticleEmitter,
  Option<&'static RenderLayers>,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  Option<&'static DrawLift>,
);

/// What the drawer reads of an emitter, in the order it draws them
#[derive(Debug, Clone, Copy)]
struct Drawn {
  order: (u32, Entity),
  slot: u32,
  unscaled: bool,
  /// Its own z, over its effect's
  z: Option<i32>,
  layers: RenderLayers,
}

/// Pushes one run per visible emitter into the frame's draw list. Kept by
/// the engine between frames, with its query state and the order buffer,
/// so a steady frame allocates nothing.
#[derive(Default)]
pub(crate) struct ParticleDrawer {
  /// Made on the first frame with live particles
  query: Option<QueryState<DrawnEmitter>>,
  order: Vec<Drawn>,
}

impl ParticleDrawer {
  /// Pushes each enabled emitter's particles into `list` as one run, in
  /// entity order: at the emitter's z (else its effect's), on its entity's
  /// `RenderLayers`, with its effect's blend, filter, lit flag and sprite
  /// effect, in spawn order (the newest on top). A pool whose box, grown by
  /// the largest a particle of its effect draws, meets no camera's view on
  /// its layers is skipped, as is one whose image draws nothing (not
  /// loaded, in a release build). Each particle sits blended between its
  /// last two steps by `alpha`, the fixed step's fraction (an unscaled
  /// pool by its own). Returns how many particles it submitted.
  pub(crate) fn collect(
    &mut self,
    world: &mut World,
    list: &mut DrawList,
    textures: &mut SpriteGpuRenderer,
    alpha: f32,
    cameras: &[CameraDraw],
  ) -> u32 {
    if world
      .get_resource::<ParticleStore>()
      .is_none_or(|store| store.live() == 0)
    {
      return 0;
    }
    let query = self
      .query
      .get_or_insert_with(|| world.query::<DrawnEmitter>());
    let world: &World = world;
    let (Some(store), Some(images)) = (
      world.get_resource::<ParticleStore>(),
      world.get_resource::<Assets<Image>>(),
    ) else {
      return 0;
    };
    self.order.clear();
    self.order.extend(
      query
        .iter(world)
        .map(|(entity, emitter, layers, ..)| Drawn {
          order: entity_order(&entity),
          slot: emitter.slot,
          unscaled: emitter.is_unscaled(),
          z: emitter.z(),
          layers: layers.copied().unwrap_or_default(),
        }),
    );
    self.order.sort_unstable_by_key(|drawn| drawn.order);

    let mut submitted = 0u32;
    for drawn in &self.order {
      let Some(pool) = store
        .pool(drawn.slot)
        .filter(|pool| pool.entity == drawn.order.1)
      else {
        continue;
      };
      let (Some((low, high)), Some(compiled)) =
        (pool.bounds, pool.effect.and_then(|id| store.compiled(id)))
      else {
        continue;
      };
      let particles = &pool.main;
      if particles.len() == 0 {
        continue;
      }
      // Grown by the largest a particle draws, so one at its edge counts
      let grow = Vec2::splat(compiled.largest_size);
      let (min, max) = (low - grow, high + grow);
      let seen = cameras
        .iter()
        .any(|camera| camera.layers.intersects(&drawn.layers) && camera.view.is_visible(min, max));
      if !seen {
        continue;
      }
      let effect = &compiled.effect;
      // The checker while its image is not loaded (debug builds), or
      // nothing, as the scenes' sprites draw
      let Some(texture) = textures.texture_of(effect.image.id(), images) else {
        continue;
      };
      let fraction = drawn_fraction(pool, drawn.unscaled, store.step, alpha);
      // A `Local` effect's particles draw through the emitter as it draws
      let local = match pool.local {
        Some(_) => match query.get(world, drawn.order.1) {
          Ok((.., global, previous, lift)) => {
            Some(LocalFrame::drawn(global, previous, lift, alpha))
          }
          Err(_) => continue,
        },
        None => None,
      };
      let instances = list.run_instances();
      let start = instances.len();
      let (Ok(first), Ok(end)) = (u32::try_from(start), u32::try_from(start + particles.len()))
      else {
        continue;
      };
      // Spawn order: the newest last, drawn on top
      instances.extend(
        (0..particles.len())
          .map(|i| instance_for(particles, i, compiled, fraction, local.as_ref())),
      );
      let shading = if effect.effect.is_some() {
        SpriteShading::Effect
      } else {
        SpriteShading::Plain
      };
      let run = DrawRun {
        texture,
        shading,
        blend: effect.blend,
        filter: effect.filter,
        layers: drawn.layers,
        min,
        max,
        instances: first..end,
      };
      list.push_run(effect.lit, drawn.z.unwrap_or(effect.z), run);
      submitted = submitted.saturating_add(end - first);
    }
    submitted
  }
}

/// How far between its last two steps `pool`'s particles draw: the frame's
/// fixed-step `alpha` for a game-time pool stepped in the store's latest
/// `step`, an unscaled pool's own fraction, and 1 (where they are) for a
/// game-time pool that was not stepped then (held, or at speed 0), whose
/// particles would otherwise sway with the frame's fraction
fn drawn_fraction(pool: &EmitterPool, unscaled: bool, step: u64, alpha: f32) -> f32 {
  if unscaled {
    pool.fraction
  } else if pool.last_step == Some(step) {
    alpha
  } else {
    1.0
  }
}

/// Particle `i` of `pool` as drawn: blended `alpha` of the way from where it
/// was the step before to where it is (through its emitter's `local` frame
/// for a `Local` effect's), a quad `size` across scaled by the effect's
/// size curve at its age, turned by its rotation, its start colour times
/// the effect's gradient channel by channel, with the effect's sprite
/// effect and emission (as the scenes' sprites take them).
pub(crate) fn instance_for(
  pool: &Pool,
  i: usize,
  compiled: &Compiled,
  alpha: f32,
  local: Option<&LocalFrame>,
) -> SpriteInstance {
  let effect = &compiled.effect;
  let t = pool.age[i] / pool.lifetime[i];
  let x = pool.px[i] + (pool.x[i] - pool.px[i]) * alpha;
  let y = pool.py[i] + (pool.y[i] - pool.py[i]) * alpha;
  let (x, y, rotation) = match local {
    Some(frame) => {
      let at = frame.world_point(Vec2::new(x, y));
      (at.x, at.y, frame.world_rotation(pool.rotation[i]))
    }
    None => (x, y, pool.rotation[i]),
  };
  let size = pool.size[i] * effect.size_over_life.lookup(t);
  let mut instance = SpriteInstance::new().with_transform(x, y, rotation, size, size);
  let (start, tint) = (pool.color[i], effect.color_over_life.lookup(t));
  instance.tint = std::array::from_fn(|c| start[c] * tint[c]);
  if let Some(sprite_effect) = &effect.effect {
    instance = instance.with_effect(sprite_effect);
  }
  instance.with_emission(effect.emission)
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;

  use glam::Vec2;

  use super::*;
  use crate::core::asset::Assets;
  use crate::core::ecs::SpriteEffect;
  use crate::core::graphics::Image;
  use crate::core::particles::effect::ParticleEffect;
  use crate::core::particles::pool::NewParticle;
  use crate::core::particles::value::{Curve, Gradient};
  use crate::core::ui::Color;

  /// The default effect as `change` leaves it, compiled
  fn compiled(change: impl FnOnce(ParticleEffect) -> ParticleEffect) -> Compiled {
    let image = Assets::<Image>::new().add(Image::solid_rect(4, 4, 255, 255, 255, 255));
    Compiled::new(&change(ParticleEffect::new(image)), 1, String::new)
  }

  /// A particle at `at`, living 2 s, 10 across, white
  fn particle(at: Vec2) -> NewParticle {
    NewParticle {
      position: at,
      velocity: Vec2::ZERO,
      lifetime: 2.0,
      size: 10.0,
      rotation: 0.0,
      spin: 0.0,
      color: [1.0; 4],
      seed: 0,
    }
  }

  /// Where `instance`'s quad is centred
  fn centre(instance: &SpriteInstance) -> Vec2 {
    Vec2::new(instance.transform[2][0], instance.transform[2][1])
  }

  /// `instance`'s x axis: its width along its turn
  fn x_axis(instance: &SpriteInstance) -> Vec2 {
    Vec2::new(instance.transform[0][0], instance.transform[0][1])
  }

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).abs().max_element() < 1e-4
  }

  #[test]
  fn a_pool_not_stepped_in_the_latest_step_draws_where_it_stands() {
    let mut pool = EmitterPool::free();
    pool.last_step = Some(7);
    assert_eq!(drawn_fraction(&pool, false, 7, 0.25), 0.25, "blended");
    assert_eq!(
      drawn_fraction(&pool, false, 8, 0.25),
      1.0,
      "held, or at speed 0: still"
    );
    // An unscaled pool's last step counts frames: it keeps its own fraction
    pool.fraction = 0.6;
    assert_eq!(drawn_fraction(&pool, true, 8, 0.25), 0.6);
    assert_eq!(drawn_fraction(&pool, true, 7, 0.25), 0.6);
  }

  #[test]
  fn an_instance_blends_between_steps() {
    let fx = compiled(|fx| fx);
    let mut pool = Pool::default();
    pool.push(&particle(Vec2::new(10.0, 20.0)));
    // It moved from (10, 20) to (30, 60) in its last step
    pool.x[0] = 30.0;
    pool.y[0] = 60.0;
    let at = |alpha| centre(&instance_for(&pool, 0, &fx, alpha, None));
    assert_eq!(at(0.5), Vec2::new(20.0, 40.0), "halfway");
    assert_eq!(at(0.0), Vec2::new(10.0, 20.0), "where it was");
    assert_eq!(at(1.0), Vec2::new(30.0, 60.0), "where it is");
  }

  #[test]
  fn a_local_instance_draws_through_its_emitter_s_frame() {
    use crate::core::ecs::{GlobalTransform, Transform};

    let fx = compiled(|fx| fx);
    let mut pool = Pool::default();
    pool.push(&NewParticle {
      rotation: 0.25,
      ..particle(Vec2::new(10.0, 0.0))
    });
    pool.x[0] = 20.0;
    // An emitter at (100, 50), turned a quarter and scaled, which never
    // stretches its frame
    let at = Transform::from_xy(100.0, 50.0)
      .with_rotation(FRAC_PI_2)
      .with_uniform_scale(3.0);
    let frame = LocalFrame::drawn(&GlobalTransform::from_transform(&at), None, None, 1.0);
    let local = |alpha| instance_for(&pool, 0, &fx, alpha, Some(&frame));
    assert!(
      close(centre(&local(1.0)), Vec2::new(100.0, 70.0)),
      "{:?}",
      local(1.0)
    );
    assert!(
      close(centre(&local(0.5)), Vec2::new(100.0, 65.0)),
      "blended in its frame"
    );
    // Turned with its emitter
    let turned = Vec2::from_angle(FRAC_PI_2 + 0.25) * 10.0;
    assert!(close(x_axis(&local(1.0)), turned), "{:?}", local(1.0));
  }

  #[test]
  fn an_instance_takes_its_life_s_size_and_colour() {
    let fx = compiled(|fx| {
      fx.with_size_over_life(Curve::new([(0.0, 1.0), (1.0, 0.2)]))
        .with_color_over_life(Gradient::new([
          (0.0, Color::WHITE),
          (1.0, Color::new(0, 128, 255, 0)),
        ]))
        .with_emission(3.0)
        .with_effect(SpriteEffect::Flash {
          color: Color::WHITE,
          intensity: 0.5,
        })
    });
    let mut pool = Pool::default();
    pool.push(&NewParticle {
      color: [0.5, 1.0, 0.25, 0.8],
      rotation: FRAC_PI_2,
      ..particle(Vec2::ZERO)
    });

    // Born: its start size and colour, turned a quarter
    let born = instance_for(&pool, 0, &fx, 1.0, None);
    assert!(close(x_axis(&born), Vec2::new(0.0, 10.0)), "{born:?}");
    assert_eq!(born.tint, [0.5, 1.0, 0.25, 0.8]);
    assert_eq!(born.effect_params[0], 3.0, "with its effect");
    assert_eq!(born.emission(), 3.0, "with its emission");

    // Halfway through its life: scaled by the curves there, channel by channel
    pool.age[0] = 1.0;
    let half = instance_for(&pool, 0, &fx, 1.0, None);
    let size = 10.0 * fx.effect.size_over_life.lookup(0.5);
    assert!((size - 6.0).abs() < 0.01, "the curve halfway: {size}");
    assert!(close(x_axis(&half), Vec2::new(0.0, size)), "{half:?}");
    let tint = fx.effect.color_over_life.lookup(0.5);
    let start = [0.5, 1.0, 0.25, 0.8];
    let want: [f32; 4] = std::array::from_fn(|c| start[c] * tint[c]);
    assert_eq!(half.tint, want);
    assert!(want[0] < 0.3 && want[3] < 0.5, "faded: {want:?}");
  }
}
