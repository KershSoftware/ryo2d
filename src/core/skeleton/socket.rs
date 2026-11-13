//! Sockets: child entities of a skeleton entity that ride its bones.

use glam::{Affine2, Vec2};

use super::data::{BoneId, DataVersion};
use super::scope::SkeletonStore;
use crate::core::ecs::kernel::{DeferredWorld, HookContext};
use crate::core::ecs::{Children, Component, Entity, PreviousGlobalTransform, Transform, World};
use crate::core::particles::ParticleEmitter;

/// Puts a child entity of a skeleton entity on one of the skeleton's
/// bones: a weapon in a hand, a light on a head, an emitter at a muzzle.
/// In every fixed step, once the skeleton has posed its bones, the engine
/// sets the entity's `Transform` (relative to the skeleton's entity, as
/// any child's is) to the bone's transform followed by `offset`, and
/// places the entity and everything below it there in the same step, so
/// nothing on the socket trails the bone.
///
/// The entity's `Transform` is the engine's while the socket is on it:
/// position it with `offset`. A bone the skeleton lacks warns once for
/// that name and leaves the entity where it is. A socket whose parent is
/// not a skeleton does nothing.
///
/// Under an interpolated skeleton the entity is drawn between steps as
/// the skeleton is. When it arrives on a bone, not having moved there with
/// it (a new socket, its skeleton's first pose, a bone found again or
/// another bone named, a socket dropped and picked up again), it is drawn
/// there at once with everything below it, and a particle emitter on any
/// of them leaves no streak along the way; and so when the skeleton's
/// entity skips interpolation (a teleport).
///
/// A socket added while no fixed step runs (the clock paused) is on its
/// bone in that frame. The bone may be renamed at any time
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, SocketState)]
#[component(on_insert = socket_inserted)]
pub struct BoneSocket {
  /// The bone's name
  pub bone: String,
  /// Where the entity sits on the bone: its place, turn and scale in the
  /// bone's own space
  pub offset: Transform,
  /// Whether the entity takes the bone's scale (and mirroring) as well as
  /// its place and direction. Without it the entity keeps the size its
  /// offset and the skeleton's entity give it, however the bone is scaled,
  /// and its offset is not stretched either
  pub inherit_scale: bool,
}

impl BoneSocket {
  /// A socket at the origin of the bone called `bone`, taking its scale
  pub fn new(bone: impl Into<String>) -> Self {
    Self {
      bone: bone.into(),
      offset: Transform::IDENTITY,
      inherit_scale: true,
    }
  }

  /// Sets where the entity sits on the bone
  pub fn with_offset(mut self, offset: Transform) -> Self {
    self.offset = offset;
    self
  }

  /// Leaves the bone's scale out: the entity takes its place and direction
  /// only
  pub fn without_scale(mut self) -> Self {
    self.inherit_scale = false;
    self
  }

  /// The entity's transform relative to its skeleton's entity, given its
  /// bone's transform in the skeleton's space: the bone's (its place and
  /// direction alone without `inherit_scale`) followed by the offset. A
  /// `Transform` has no shear, so a skew in the result is dropped: it
  /// points along the result's x axis, as long as that axis, and as tall
  /// as the result is across it (negative when mirrored)
  pub(crate) fn local(&self, bone: Affine2) -> Transform {
    let bone = if self.inherit_scale {
      bone
    } else {
      Affine2::from_angle_translation(bone.matrix2.x_axis.to_angle(), bone.translation)
    };
    let placed = bone * self.offset.to_affine();
    let along = placed.matrix2.x_axis;
    let length = along.length();
    let across = if length > 0.0 {
      placed.matrix2.determinant() / length
    } else {
      placed.matrix2.y_axis.length()
    };
    Transform {
      translation: placed.translation,
      rotation: along.to_angle(),
      scale: Vec2::new(length, across),
    }
  }
}

/// What the engine keeps of a socket between steps, beside it on its
/// entity. A `BoneSocket` put on the entity starts it afresh
#[derive(Component, Debug, Clone, Default)]
pub(crate) struct SocketState {
  /// The bone last looked up
  pub(crate) found: Option<FoundBone>,
  /// The skeleton entity whose bone it stands on, and the number of that
  /// skeleton's placing that last put it there; `None` while it is not
  /// placed
  pub(crate) on: Option<(Entity, u64)>,
  /// The name of a missing bone it has been told about
  pub(crate) told: Option<Box<str>>,
}

/// A socket's bone as last looked up: the name asked for, the data it was
/// looked up in (id and version), and the bone's place there, if that data
/// has it. The name is looked up again only when the data's version or the
/// socket's name is another: both are compared as they are, each time, so
/// a change made while nothing placed the socket is still seen
#[derive(Debug, Clone)]
pub(crate) struct FoundBone {
  pub(crate) name: Box<str>,
  pub(crate) seen: DataVersion,
  pub(crate) bone: Option<BoneId>,
}

/// A socket was added, or put over another: what was kept of the one
/// before is forgotten, so it is placed as a new socket is; and the store
/// counts it, so the frame's pass looks for it
fn socket_inserted(mut world: DeferredWorld, context: HookContext) {
  if let Some(mut state) = world.get_mut::<SocketState>(context.entity) {
    *state = SocketState::default();
  }
  if let Some(mut store) = world.get_resource_mut::<SkeletonStore>() {
    store.add_socket();
  }
}

/// A skeleton's entity jumped (`skip_interpolation`): its sockets, and
/// everything below them, are drawn where they stand until the next step,
/// as it is, and an emitter on any of them spawns only where it lands, as
/// the entity's own does. They ride its bones, so they jump with it
pub(crate) fn skip_sockets(world: &mut World, skeleton: Entity) {
  let below = |world: &World, entity: Entity| -> Vec<Entity> {
    world
      .get::<Children>(entity)
      .map(|children| children.iter().copied().collect())
      .unwrap_or_default()
  };
  let mut walk = below(world, skeleton);
  walk.retain(|&child| world.get::<BoneSocket>(child).is_some());
  while let Some(entity) = walk.pop() {
    if let Some(mut previous) = world.get_mut::<PreviousGlobalTransform>(entity) {
      previous.skip = true;
    }
    if let Some(mut emitter) = world.get_mut::<ParticleEmitter>(entity) {
      emitter.teleported = true;
    }
    walk.extend(below(world, entity));
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn same(actual: Affine2, expected: Affine2) {
    assert!(
      actual.abs_diff_eq(expected, 1e-4),
      "expected {expected:?}, got {actual:?}"
    );
  }

  #[test]
  fn a_socket_takes_its_bone_s_transform_then_its_offset() {
    let offset = Transform::from_xy(4.0, -2.0)
      .with_rotation(0.5)
      .with_scale(Vec2::new(1.5, 1.5));
    let socket = BoneSocket::new("hand").with_offset(offset);
    // Turned, scaled evenly, and mirrored: all of it is kept exactly
    for scale in [
      Vec2::new(2.0, 2.0),
      Vec2::new(-2.0, 2.0),
      Vec2::new(2.0, -2.0),
    ] {
      let bone = Affine2::from_scale_angle_translation(scale, 1.2, Vec2::new(30.0, 10.0));
      same(socket.local(bone).to_affine(), bone * offset.to_affine());
    }
    // An uneven scale is kept too while the offset does not turn across it
    let straight = BoneSocket::new("hand").with_offset(Transform::from_xy(4.0, -2.0));
    let bone = Affine2::from_scale_angle_translation(Vec2::new(3.0, 0.5), -0.7, Vec2::ZERO);
    same(
      straight.local(bone).to_affine(),
      bone * Transform::from_xy(4.0, -2.0).to_affine(),
    );
  }

  #[test]
  fn without_scale_a_socket_takes_its_bone_s_place_and_direction_only() {
    let offset = Transform::from_xy(4.0, 0.0).with_rotation(0.5);
    let socket = BoneSocket::new("hand").with_offset(offset).without_scale();
    let bone =
      Affine2::from_scale_angle_translation(Vec2::new(3.0, 0.5), 1.2, Vec2::new(30.0, 10.0));
    let local = socket.local(bone);
    // 4 along the bone's direction from its origin, unstretched, at the
    // bone's angle plus the offset's, at the offset's own scale
    let along = Vec2::from_angle(1.2);
    assert!(
      local
        .translation
        .abs_diff_eq(Vec2::new(30.0, 10.0) + along * 4.0, 1e-4)
    );
    assert!((local.rotation - 1.7).abs() < 1e-5);
    assert!(local.scale.abs_diff_eq(Vec2::ONE, 1e-5));
  }

  #[test]
  fn a_skewed_bone_s_socket_keeps_its_direction_and_drops_the_skew() {
    // A bone whose axes are not square to each other (a turned bone under
    // an unevenly scaled one): x axis (2, 0), y axis (1, 1)
    let bone = Affine2::from_cols(
      Vec2::new(2.0, 0.0),
      Vec2::new(1.0, 1.0),
      Vec2::new(5.0, 6.0),
    );
    let local = BoneSocket::new("hand").local(bone);
    assert_eq!(local.translation, Vec2::new(5.0, 6.0));
    assert_eq!(local.rotation, 0.0);
    // As long as the x axis, and as tall as the bone is across it (its
    // area, 2, over that length)
    assert!(local.scale.abs_diff_eq(Vec2::new(2.0, 1.0), 1e-6));
    // A bone squashed flat gives no direction and no NaN
    let flat = BoneSocket::new("hand").local(Affine2::from_scale(Vec2::ZERO));
    assert_eq!(flat, Transform::IDENTITY.with_scale(Vec2::ZERO));
  }
}
