//! Ready-made trigger behaviours. Each goes on a trigger collider and acts
//! in `FixedSet::Movement` on the previous step's `TriggerEntered` messages;
//! what can set it off is the collider's layer and mask.

use super::events::{CheckpointReached, Killed, PickedUp};
use super::platformer::PlatformerState;
use super::top_down::TopDownState;
use crate::core::ecs::{
  Added, Allow, Commands, Component, Disabled, Entity, EntityHashSet, GlobalTransform, Knockback,
  Local, MessageReader, MessageWriter, Or, Query, Res, Transform, Vec2, Velocity, With,
};
use crate::core::physics::{Collider, TriggerEntered};
use crate::core::time::Time;

/// Collected by the first thing its mask lets in: `PickedUp` is written and
/// the pickup despawned, or disabled and brought back after `respawn_after`
/// seconds.
#[derive(Component, Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Pickup {
  pub respawn_after: Option<f32>,
  pub(crate) timer: Option<f32>,
}

impl Pickup {
  pub fn new() -> Self {
    Self::default()
  }

  /// Come back `seconds` after being collected (arcade coins, power-ups).
  pub fn respawn_after(mut self, seconds: f32) -> Self {
    self.respawn_after = Some(seconds);
    self
  }
}

/// Where a `KillZone` puts this entity back; a `Checkpoint` moves it. Put it
/// on root entities (it sets `Transform.translation`).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Respawn {
  pub point: Vec2,
  pub(crate) placed: bool,
}

impl Respawn {
  /// The entity's position when the component is added.
  pub fn here() -> Self {
    Self {
      point: Vec2::ZERO,
      placed: false,
    }
  }

  pub fn at(point: Vec2) -> Self {
    Self {
      point,
      placed: true,
    }
  }
}

/// Writes `Killed` for what enters. An entity with `Respawn` is put back at
/// its point with its velocity, knockback and preset state cleared; others
/// only get the message, unless the zone is `despawning()`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct KillZone {
  pub despawn: bool,
}

impl KillZone {
  pub fn new() -> Self {
    Self::default()
  }

  /// Despawn what enters without a `Respawn` (bullets, enemies falling out).
  pub fn despawning(mut self) -> Self {
    self.despawn = true;
    self
  }
}

/// Moves the `Respawn` point of what enters to the checkpoint's position
/// plus `offset`; writes `CheckpointReached` when the point changes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct Checkpoint {
  pub offset: Vec2,
}

impl Checkpoint {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_offset(mut self, offset: Vec2) -> Self {
    self.offset = offset;
    self
  }
}

/// Give `Respawn::here()` its entity's position: its `Transform`, which is
/// what a respawn writes back (a world position on a root entity), and which
/// is set on the step it was spawned, unlike `GlobalTransform`.
pub(crate) fn place_respawn_points(mut points: Query<(&mut Respawn, &Transform)>) {
  for (mut respawn, transform) in &mut points {
    if !respawn.placed {
      respawn.point = transform.translation;
      respawn.placed = true;
    }
  }
}

type Behaviours = Or<(Added<Pickup>, Added<KillZone>, Added<Checkpoint>)>;

/// A behaviour on a collider that is not a trigger never fires: say so once.
pub(crate) fn warn_untriggered(
  added: Query<(Entity, Option<&Collider>), Behaviours>,
  mut warned: Local<EntityHashSet>,
) {
  for (entity, collider) in &added {
    if !collider.is_some_and(|collider| collider.is_trigger) && warned.insert(entity) {
      crate::engine_warn!(
        module: "Gameplay",
        "{entity}: Pickup, KillZone and Checkpoint need a trigger collider (Collider::as_trigger) on the same entity"
      );
    }
  }
}

pub(crate) fn collect_pickups(
  mut entered: MessageReader<TriggerEntered>,
  mut pickups: Query<&mut Pickup>,
  mut picked: MessageWriter<PickedUp>,
  mut commands: Commands,
) {
  let mut collected = EntityHashSet::default();
  for message in entered.read() {
    let Ok(mut pickup) = pickups.get_mut(message.trigger) else {
      continue;
    };
    if !collected.insert(message.trigger) {
      continue;
    }
    picked.write(PickedUp {
      pickup: message.trigger,
      by: message.actor(),
    });
    match pickup.respawn_after {
      Some(seconds) => {
        pickup.timer = Some(seconds);
        commands.entity(message.trigger).insert(Disabled);
      }
      None => commands.entity(message.trigger).despawn(),
    }
  }
}

/// Pickups waiting out their respawn time (disabled, so only `Allow` sees them).
type Waiting = (With<Disabled>, Allow<Disabled>);

pub(crate) fn respawn_pickups(
  time: Res<Time>,
  mut waiting: Query<(Entity, &mut Pickup), Waiting>,
  mut commands: Commands,
) {
  let dt = time.fixed.timestep();
  for (entity, mut pickup) in &mut waiting {
    let Some(left) = pickup.timer else {
      continue;
    };
    if left <= dt {
      pickup.timer = None;
      commands.entity(entity).remove::<Disabled>();
    } else {
      pickup.timer = Some(left - dt);
    }
  }
}

type Respawned = (
  &'static Respawn,
  &'static mut Transform,
  Option<&'static mut Velocity>,
  Option<&'static mut Knockback>,
  Option<&'static mut PlatformerState>,
  Option<&'static mut TopDownState>,
);

pub(crate) fn apply_kill_zones(
  mut entered: MessageReader<TriggerEntered>,
  zones: Query<&KillZone>,
  mut actors: Query<Respawned>,
  mut killed: MessageWriter<Killed>,
  mut commands: Commands,
) {
  for message in entered.read() {
    let Ok(zone) = zones.get(message.trigger) else {
      continue;
    };
    let entity = message.actor();
    killed.write(Killed {
      entity,
      zone: message.trigger,
    });
    match actors.get_mut(entity) {
      Ok((respawn, mut transform, velocity, knockback, platformer, top_down)) => {
        transform.translation = respawn.point;
        if let Some(mut velocity) = velocity {
          *velocity = Velocity::zero();
        }
        if let Some(mut knockback) = knockback {
          knockback.stop();
        }
        if let Some(mut state) = platformer {
          state.reset();
        }
        if let Some(mut state) = top_down {
          state.reset();
        }
      }
      Err(_) if zone.despawn => {
        if let Ok(mut actor) = commands.get_entity(entity) {
          actor.despawn();
        }
      }
      Err(_) => {}
    }
  }
}

pub(crate) fn apply_checkpoints(
  mut entered: MessageReader<TriggerEntered>,
  checkpoints: Query<(&Checkpoint, &GlobalTransform)>,
  mut respawns: Query<&mut Respawn>,
  mut reached: MessageWriter<CheckpointReached>,
) {
  for message in entered.read() {
    let Ok((checkpoint, global)) = checkpoints.get(message.trigger) else {
      continue;
    };
    let entity = message.actor();
    let Ok(mut respawn) = respawns.get_mut(entity) else {
      continue;
    };
    let point = global.translation() + checkpoint.offset;
    if !respawn.placed || respawn.point.distance(point) > 0.5 {
      respawn.point = point;
      respawn.placed = true;
      reached.write(CheckpointReached {
        checkpoint: message.trigger,
        entity,
      });
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Messages, Transform, Velocity, World};
  use crate::core::gameplay::test_util::*;
  use crate::core::physics::{Collider, RigidBody};

  fn read<M: crate::core::ecs::Message + Copy>(world: &World) -> Vec<M> {
    let messages = world.resource::<Messages<M>>();
    messages.get_cursor().read(messages).copied().collect()
  }

  fn zone(world: &mut World, x: f32, behaviour: impl crate::core::ecs::Bundle) -> Entity {
    world
      .spawn((
        Transform::from_xy(x, 0.0),
        Collider::rect(40.0, 200.0).as_trigger(),
        behaviour,
      ))
      .id()
  }

  fn mover(world: &mut World, x: f32, vx: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(x, 0.0),
        RigidBody::kinematic(),
        Collider::rect(10.0, 10.0),
        Velocity::new(vx, 0.0),
      ))
      .id()
  }

  #[test]
  fn a_pickup_is_collected_once_and_despawned() {
    let (mut world, mut s) = world(60);
    let coin = zone(&mut world, 60.0, Pickup::new());
    let player = mover(&mut world, 0.0, 300.0);
    run(&mut world, &mut s, 20);
    assert_eq!(
      read::<PickedUp>(&world),
      [PickedUp {
        pickup: coin,
        by: player
      }]
    );
    assert!(world.get_entity(coin).is_err());
  }

  // Review focus 4
  #[test]
  fn two_collectors_in_one_step_collect_once() {
    let (mut world, mut s) = world(60);
    let coin = zone(&mut world, 0.0, Pickup::new());
    mover(&mut world, -2.0, 0.0);
    mover(&mut world, 2.0, 0.0);
    run(&mut world, &mut s, 5);
    let picked = read::<PickedUp>(&world);
    assert_eq!(picked.len(), 1, "{picked:?}");
    assert_eq!(picked[0].pickup, coin);
  }

  #[test]
  fn a_respawning_pickup_comes_back() {
    let (mut world, mut s) = world(60);
    let coin = zone(&mut world, 60.0, Pickup::new().respawn_after(0.5));
    mover(&mut world, 0.0, 300.0);
    run(&mut world, &mut s, 20);
    assert!(world.get::<crate::core::ecs::Disabled>(coin).is_some());
    run(&mut world, &mut s, 40);
    assert!(world.get::<crate::core::ecs::Disabled>(coin).is_none());
  }

  #[test]
  fn a_pickup_the_game_disabled_stays_disabled() {
    use crate::core::ecs::Disabled;
    let (mut world, mut s) = world(60);
    let spawned = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        Pickup::new().respawn_after(0.5),
        Disabled,
      ))
      .id();
    let later = world
      .spawn((
        Transform::from_xy(50.0, 0.0),
        Pickup::new().respawn_after(0.5),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.entity_mut(later).insert(Disabled);
    run(&mut world, &mut s, 60);
    assert!(world.get::<Disabled>(spawned).is_some());
    assert!(world.get::<Disabled>(later).is_some());
  }

  #[test]
  fn a_kill_zone_respawns_an_entity_with_respawn_and_clears_its_motion() {
    let (mut world, mut s) = world(60);
    let pit = zone(&mut world, 60.0, KillZone::new());
    let player = mover(&mut world, 0.0, 300.0);
    world
      .entity_mut(player)
      .insert(Respawn::at(Vec2::new(-200.0, 0.0)));
    run(&mut world, &mut s, 20);
    assert_eq!(
      read::<Killed>(&world)[0],
      Killed {
        entity: player,
        zone: pit
      }
    );
    assert!(at(&world, player).translation.x < -150.0);
    assert_eq!(*world.get::<Velocity>(player).unwrap(), Velocity::zero());
  }

  #[test]
  fn a_kill_zone_only_reports_others_unless_it_despawns_them() {
    let (mut world, mut s) = world(60);
    zone(&mut world, 60.0, KillZone::new());
    zone(&mut world, -60.0, KillZone::new().despawning());
    let kept = mover(&mut world, 0.0, 300.0);
    let gone = mover(&mut world, 0.0, -300.0);
    run(&mut world, &mut s, 20);
    assert_eq!(read::<Killed>(&world).len(), 2);
    assert!(world.get_entity(kept).is_ok());
    assert!(world.get_entity(gone).is_err());
  }

  #[test]
  fn a_checkpoint_moves_the_respawn_point_and_says_so_once() {
    let (mut world, mut s) = world(60);
    let flag = zone(
      &mut world,
      60.0,
      Checkpoint::new().with_offset(Vec2::new(0.0, -20.0)),
    );
    let player = mover(&mut world, 0.0, 120.0);
    world.entity_mut(player).insert(Respawn::here());
    run(&mut world, &mut s, 60);
    assert_eq!(
      world.get::<Respawn>(player).unwrap().point,
      Vec2::new(60.0, -20.0)
    );
    assert_eq!(
      read::<CheckpointReached>(&world),
      [CheckpointReached {
        checkpoint: flag,
        entity: player
      }]
    );
  }

  #[test]
  fn respawn_here_takes_the_position_when_added() {
    let (mut world, mut s) = world(60);
    let player = mover(&mut world, 33.0, 0.0);
    world.entity_mut(player).insert(Respawn::here());
    run(&mut world, &mut s, 1);
    assert_eq!(
      world.get::<Respawn>(player).unwrap().point,
      Vec2::new(33.0, 0.0)
    );
  }
}
