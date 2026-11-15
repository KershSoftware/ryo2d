//! Applying a skeleton's constraints: the pass a step runs once every bone
//! has its place in the skeleton's space, the game's overrides included.
//! Each constraint, in the data's order, changes its bones, and the bones
//! it moved are placed again before the next is read. The module notes
//! give the rules.

use glam::{Affine2, Vec2};

use super::component::{SetOverride, apply_override, invertible};
use super::constraint::{Constraint, IkConstraint, TransformConstraint, TransformMix};
use super::data::{BoneData, BoneId, BoneSet, SkeletonData};
use super::ik::{IkValues, chain, one_bone, two_bones};
use super::pose::{BonePose, ConstraintValues, Pose};
use super::transform_constraint::{in_its_own_values, in_the_world};

/// What a constraint pass needs of the data, worked out once as the data
/// is built: for each constraint the bones to place again after it, for a
/// transform constraint its bones in bone order, and the bones any
/// constraint changes the own values of
#[derive(Debug, Default)]
pub(crate) struct ConstraintBones {
  /// Each constraint's bones to place again, one constraint's after
  /// another's, each in bone order. A transform constraint's are its
  /// bones', one bone's after another's
  again: Vec<BoneId>,
  /// Where each constraint's are in `again`: from, and to
  spans: Vec<(usize, usize)>,
  /// Each transform constraint's bones, one constraint's after another's,
  /// each in bone order, with where the bones to place again after that
  /// bone are in `again`
  parts: Vec<(BoneId, (usize, usize))>,
  /// Where each constraint's are in `parts` (an IK constraint has none)
  part_spans: Vec<(usize, usize)>,
  /// The bones an IK constraint or a local transform constraint changes,
  /// in bone order, each once
  changed: Vec<BoneId>,
}

impl ConstraintBones {
  /// The lists for `constraints` over `bones`, which come parents first.
  /// After an IK constraint its bones and every bone below them are placed
  /// again. After each bone of a transform constraint, the bones below
  /// that bone are, and in a local mode that bone too
  pub(super) fn of(bones: &[BoneData], constraints: &[Constraint]) -> Self {
    let id = |bone: usize| u8::try_from(bone).ok().map(BoneId);
    let mut lists = Self::default();
    let mut changed = BoneSet::default();
    for constraint in constraints {
      let (ik, local) = match constraint {
        Constraint::Ik(_) => (true, true),
        Constraint::Transform(transform) => (false, transform.setup.local),
      };
      let mut own = BoneSet::default();
      for bone in constraint.bones() {
        own.set(bone.index());
        if local {
          changed.set(bone.index());
        }
      }
      let from = lists.again.len();
      let parts_from = lists.parts.len();
      if ik {
        lists.again.extend(below(bones, &own, true).filter_map(id));
      } else {
        // Each of its bones, in bone order, with the bones below it
        for bone in (0..bones.len()).filter(|&bone| own.has(bone)) {
          let mut this = BoneSet::default();
          this.set(bone);
          let start = lists.again.len();
          lists
            .again
            .extend(below(bones, &this, local).filter_map(id));
          if let Some(bone) = id(bone) {
            lists.parts.push((bone, (start, lists.again.len())));
          }
        }
      }
      lists.spans.push((from, lists.again.len()));
      lists.part_spans.push((parts_from, lists.parts.len()));
    }
    let every = 0..bones.len();
    lists
      .changed
      .extend(every.filter(|&bone| changed.has(bone)).filter_map(id));
    lists
  }

  /// The bones to place again after constraint `constraint`, in bone order
  fn again(&self, constraint: usize) -> &[BoneId] {
    let span = self.spans.get(constraint);
    span.map_or(&[], |&span| self.listed(span))
  }

  /// A transform constraint's bones, in bone order, each with where the
  /// bones to place again after it are
  fn parts(&self, constraint: usize) -> &[(BoneId, (usize, usize))] {
    let span = self.part_spans.get(constraint);
    span
      .and_then(|&(from, to)| self.parts.get(from..to))
      .unwrap_or(&[])
  }

  /// The bones in `again` from, and to
  fn listed(&self, (from, to): (usize, usize)) -> &[BoneId] {
    self.again.get(from..to).unwrap_or(&[])
  }
}

/// The bones below those in `own`, in bone order, and those in `own` too
/// when `with_own`. A bone is below them when its parent is one of them or
/// is below them, and parents come first: one walk finds them all
fn below<'a>(
  bones: &'a [BoneData],
  own: &'a BoneSet,
  with_own: bool,
) -> impl Iterator<Item = usize> + 'a {
  let mut under = BoneSet::default();
  bones.iter().enumerate().filter_map(move |(bone, data)| {
    let parent = data.parent.map(BoneId::index);
    if parent.is_some_and(|parent| own.has(parent) || under.has(parent)) {
      under.set(bone);
    }
    (under.has(bone) || (with_own && own.has(bone))).then_some(bone)
  })
}

/// What a skeleton keeps to apply its data's constraints, sized once for
/// the data: nothing in it for data without constraints
#[derive(Debug, Clone, Default)]
pub(crate) struct Constrained {
  /// The own values, as animation left them, of each bone a constraint
  /// changes
  animated: Vec<(BoneId, BonePose)>,
  /// The bones a constraint has changed so far in the pass under way
  changed: BoneSet,
  /// A chain's bones, copied out of the pose to be solved together
  chain: Vec<BonePose>,
  /// A chain's solve's scratch
  joints: Vec<Vec2>,
}

impl Constrained {
  /// What a skeleton of `data` keeps, with room for every pass: the bones
  /// its constraints change, and the scratch its longest chain is solved
  /// in (a solve fills three entries a bone and one more)
  pub(super) fn new(data: &SkeletonData) -> Self {
    let bones = data.bones();
    let changed = &data.index().constraint_bones.changed;
    let setup = |&bone: &BoneId| Some((bone, BonePose::setup(bones.get(bone.index())?)));
    // One bone and two are solved where they are in the pose
    let chains = data
      .constraints()
      .iter()
      .filter_map(|constraint| match constraint {
        Constraint::Ik(ik) if ik.bones.len() > 2 => Some(ik.bones.len()),
        _ => None,
      });
    let (chain, joints) = match chains.max() {
      Some(longest) => (
        Vec::with_capacity(longest),
        Vec::with_capacity(super::ik::chain_room(longest)),
      ),
      None => (Vec::new(), Vec::new()),
    };
    Self {
      animated: changed.iter().filter_map(setup).collect(),
      changed: BoneSet::default(),
      chain,
      joints,
    }
  }

  /// Keeps the own values of the bones that constraints change, as
  /// animation has just left them: before any override or constraint
  /// writes them
  pub(super) fn keep(&mut self, pose: &Pose) {
    for (bone, animated) in &mut self.animated {
      if let Some(own) = pose.bones.get(bone.index()) {
        *animated = *own;
      }
    }
  }

  /// Puts those bones back as animation left them, for a pass to start
  /// from
  pub(super) fn restore(&self, pose: &mut Pose) {
    for (bone, animated) in &self.animated {
      if let Some(own) = pose.bones.get_mut(bone.index()) {
        *own = *animated;
      }
    }
  }
}

/// Applies `data`'s constraints to `pose`, in their order. Every bone must
/// have been placed in the skeleton's space, the game's `overrides`
/// applied, and the bones that constraints change must hold the own values
/// animation and those overrides left them. Each constraint changes its
/// bones, and the bones it moved are placed again before the next
/// constraint is read: after an IK constraint its bones and every bone
/// below them, from their own values.
///
/// A bone placed again that the game overrides, and that no constraint
/// has changed in this pass, has its override applied afresh, from its
/// animated values and against the pose as it now stands (`root` takes the
/// skeleton's space to the world, for a world-space one). A bone a
/// constraint has changed keeps what the constraint gave it. An IK
/// constraint whose mix is not above 0 changes nothing.
///
/// A transform constraint takes its bones in bone order, each set, and the
/// bones below it placed again, before the next is read. In a world mode
/// it sets the bone's transform in the skeleton's space and places the
/// bones below it again, leaving the bone's own values alone and the bone
/// unmarked; in a local mode it changes the bone's own values, marks it,
/// and places it and the bones below it again. One whose mixes are all 0,
/// or one of them not a number, changes nothing
pub(crate) fn constrain(
  pose: &mut Pose,
  data: &SkeletonData,
  root: Affine2,
  overrides: &[SetOverride],
  state: &mut Constrained,
) {
  let bones = data.bones().len();
  if pose.bones.len() != bones || pose.skeleton_space.len() != bones {
    // A pose of other data: its bones are not these constraints'
    return;
  }
  let lists = &data.index().constraint_bones;
  state.changed = BoneSet::default();
  for (at, constraint) in data.constraints().iter().enumerate() {
    match (constraint, pose.constraints.get(at).copied()) {
      (Constraint::Ik(ik), Some(ConstraintValues::Ik(values))) => {
        // At a mix of 0, or one that is not a number, it is passed over as
        // under a flat parent: nothing changed, marked or placed again, so
        // its bones are left to the game's overrides
        let applies = values.mix > 0.0;
        if !applies || !solve(pose, data, ik, values, state) {
          continue;
        }
        for bone in &ik.bones {
          state.changed.set(bone.index());
        }
        place_listed(pose, data, root, lists.again(at), overrides, &state.changed);
      }
      (Constraint::Transform(transform), Some(ConstraintValues::Transform(mix))) => {
        // Passed over at every mix 0 as an IK constraint is at a mix of 0
        if !applies(&mix) {
          continue;
        }
        // Each bone, in bone order, is set and the bones below it placed
        // again before the next is read
        for &(bone, after) in lists.parts(at) {
          copy(pose, transform, bone, &mix, state);
          place_listed(
            pose,
            data,
            root,
            lists.listed(after),
            overrides,
            &state.changed,
          );
        }
      }
      _ => {}
    }
  }
}

/// Whether a transform constraint at `mix` does anything: each mix a
/// number, and one of them not 0
fn applies(mix: &TransformMix) -> bool {
  let mixes = [
    mix.rotate,
    mix.x,
    mix.y,
    mix.scale_x,
    mix.scale_y,
    mix.shear_y,
  ];
  mixes.iter().all(|mix| mix.is_finite()) && mixes.iter().any(|&mix| mix != 0.0)
}

/// Moves one bone of a transform constraint toward its target, as its
/// setup says: in the skeleton's space, or in its own values, which marks
/// it changed
fn copy(
  pose: &mut Pose,
  transform: &TransformConstraint,
  bone: BoneId,
  mix: &TransformMix,
  state: &mut Constrained,
) {
  let (setup, target) = (&transform.setup, transform.target.index());
  if setup.local {
    let target = pose.bones.get(target).copied();
    if let Some(target) = target
      && let Some(own) = pose.bones.get_mut(bone.index())
    {
      in_its_own_values(own, &target, mix, &setup.offset, setup.relative);
      state.changed.set(bone.index());
    }
  } else {
    let target = pose.skeleton_space.get(target).copied();
    if let Some(target) = target
      && let Some(placed) = pose.skeleton_space.get_mut(bone.index())
    {
      // Its own values are as they were, and it is not marked: placed
      // again later, it is placed from them, its override afresh
      in_the_world(placed, &target, mix, &setup.offset, setup.relative);
    }
  }
}

/// Solves one IK constraint, writing its bones' own values. False, with
/// nothing written, when it cannot be applied: its target cannot be
/// brought into the space of its first bone's parent, which is flat (a
/// scale of 0 above the bones), so the bones stay as animated; or the pose
/// lacks one of its bones
fn solve(
  pose: &mut Pose,
  data: &SkeletonData,
  ik: &IkConstraint,
  values: IkValues,
  state: &mut Constrained,
) -> bool {
  let (Some(first), Some(last)) = (ik.bones.first(), ik.bones.last()) else {
    return false;
  };
  let target = pose.skeleton_space.get(ik.target.index());
  let tip = data.bones().get(last.index());
  let (Some(target), Some(tip)) = (target, tip) else {
    return false;
  };
  if first.index() >= pose.bones.len() {
    return false;
  }
  let parent = pose.parent_space(data, first.index());
  if !invertible(parent.matrix2) {
    return false;
  }
  let target = parent.inverse().transform_point2(target.translation);
  let (length, uniform) = (tip.length, ik.setup.uniform);
  match *ik.bones {
    [bone] => {
      let Some(bone) = pose.bones.get_mut(bone.index()) else {
        return false;
      };
      one_bone(bone, length, target, values, uniform);
    }
    [first, second] => {
      let own = |bone: BoneId| pose.bones.get(bone.index()).copied();
      let (Some(mut upper), Some(mut lower)) = (own(first), own(second)) else {
        return false;
      };
      two_bones(&mut upper, &mut lower, length, target, values, uniform);
      for (bone, solved) in [(first, upper), (second, lower)] {
        if let Some(own) = pose.bones.get_mut(bone.index()) {
          *own = solved;
        }
      }
    }
    _ => {
      // A chain's bones need not be next to one another in the pose: they
      // are solved together in scratch, and their rotations copied back
      let own = |bone: &BoneId| pose.bones.get(bone.index()).copied();
      state.chain.clear();
      state.chain.extend(ik.bones.iter().filter_map(own));
      if state.chain.len() != ik.bones.len() {
        return false;
      }
      chain(
        &mut state.chain,
        length,
        target,
        values.mix,
        &mut state.joints,
      );
      for (bone, solved) in ik.bones.iter().zip(&state.chain) {
        if let Some(own) = pose.bones.get_mut(bone.index()) {
          own.rotation = solved.rotation;
        }
      }
    }
  }
  true
}

/// Places `bones` again, which come in bone order, as the `overrides` do.
/// One the game overrides, and that no constraint has `changed`, is first
/// put back to its animated values and has its override applied to them
/// afresh, against the pose as it now stands
fn place_listed(
  pose: &mut Pose,
  data: &SkeletonData,
  root: Affine2,
  bones: &[BoneId],
  overrides: &[SetOverride],
  changed: &BoneSet,
) {
  let mut next = 0;
  for bone in bones.iter().map(|bone| bone.index()) {
    if bone >= pose.bones.len() {
      continue;
    }
    let of = |set: &SetOverride| set.id.map(BoneId::index);
    while overrides
      .get(next)
      .and_then(of)
      .is_some_and(|set| set < bone)
    {
      next += 1;
    }
    if let Some(set) = overrides.get(next)
      && of(set) == Some(bone)
      && !changed.has(bone)
      && let Some(animated) = set.animated
      && let Some(own) = pose.bones.get_mut(bone)
    {
      *own = animated;
      apply_override(pose, data, bone, &set.value, root);
    }
    pose.place_bone(data, bone);
  }
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;
  use std::sync::Arc;

  use super::*;
  use crate::core::asset::{Assets, Handle};
  use crate::core::ecs::World;
  use crate::core::skeleton::apply::shortest;
  use crate::core::skeleton::component::{Fired, Flip, Space, Stand};
  use crate::core::skeleton::data::DataVersion;
  use crate::core::skeleton::ik::{CHAIN_NEAR, IkValues};
  use crate::core::skeleton::pose::ConstraintValues;
  use crate::core::skeleton::{
    Animation, BoneOverride, BoneSetup, Curve, IkSetup, Inherit, Skeleton, SkeletonAnimation,
    SkeletonDataBuilder, Timeline, TransformMix, TransformOffset, TransformSetup,
  };
  use crate::testing::asset_world;

  /// A skeleton's data in a store, for skeletons of it to be stepped
  struct Rig {
    world: World,
    handle: Handle<SkeletonData>,
  }

  impl Rig {
    fn new(data: SkeletonData) -> Self {
      let mut world = asset_world();
      let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
      Self { world, handle }
    }

    fn data(&self) -> &SkeletonData {
      let assets = self.world.resource::<Assets<SkeletonData>>();
      assets.get(&self.handle).expect("the data")
    }

    fn seen(&self) -> DataVersion {
      let assets = self.world.resource::<Assets<SkeletonData>>();
      let version = assets.version(&self.handle).expect("a version");
      (self.handle.id(), version)
    }

    /// Puts other data in the store, as a reload does
    fn reload(&mut self, data: SkeletonData) {
      let mut assets = self.world.resource_mut::<Assets<SkeletonData>>();
      *assets.get_mut(&self.handle).expect("the data") = data;
    }

    /// A skeleton of the data, not yet stepped
    fn skeleton(&self) -> Skeleton {
      Skeleton::new(self.handle.clone())
    }

    /// One fixed step of `skeleton` in its setup pose, standing as `stand`
    /// says
    fn step(&self, skeleton: &mut Skeleton, stand: impl Into<Stand>) {
      let (data, seen, fired) = (self.data(), self.seen(), &mut Fired::default());
      skeleton.step(data, seen, None, 1.0 / 60.0, stand.into(), fired);
    }

    /// One fixed step of `skeleton` as `animation` poses it
    fn step_playing(
      &self,
      skeleton: &mut Skeleton,
      animation: &mut SkeletonAnimation,
      stand: impl Into<Stand>,
    ) {
      let (data, seen, fired) = (self.data(), self.seen(), &mut Fired::default());
      skeleton.step(data, seen, Some(animation), 1.0 / 60.0, stand.into(), fired);
    }

    /// A skeleton stepped once under `root`, with `set` applied first
    fn posed(&self, root: Affine2, set: impl FnOnce(&mut Skeleton)) -> Skeleton {
      let mut skeleton = self.skeleton();
      set(&mut skeleton);
      self.step(&mut skeleton, root);
      skeleton
    }
  }

  /// A bone at (x, y) in its parent
  fn at(x: f32, y: f32) -> BoneSetup {
    BoneSetup {
      x,
      y,
      ..BoneSetup::default()
    }
  }

  /// A bone `length` long, `x` along its parent
  fn long(x: f32, length: f32) -> BoneSetup {
    BoneSetup {
      x,
      length,
      ..BoneSetup::default()
    }
  }

  /// The bones of [`limb`], without its constraint
  fn limb_bones(hips: BoneSetup, goal: Vec2) -> SkeletonDataBuilder {
    SkeletonData::builder()
      .bone("hips", None, hips)
      .bone("upper", Some("hips"), long(0.0, 40.0))
      .bone("lower", Some("upper"), long(40.0, 30.0))
      .bone("hand", Some("lower"), at(30.0, 0.0))
      .bone("goal", None, at(goal.x, goal.y))
  }

  /// A limb of two parts, 40 and 30 long, lying along x from the origin of
  /// its parent bone "hips": "upper", its child "lower", and "hand" at the
  /// limb's tip. Its target "goal" is a bone of its own, at `goal` in the
  /// skeleton's space, and the constraint "reach" brings the tip to it.
  ///
  /// The parts and a target 50 from the limb's origin make a 3-4-5
  /// triangle, with its right angle at the joint: the joint is 32 along
  /// the line to the target and 24 to the side of it. Bent the positive
  /// way under an unmirrored "hips" at the origin, a target at (0, 50)
  /// puts the joint at (24, 32), and one at (30, 40) puts it at
  /// (38.4, 11.2): from the origin to the joint and on to the target turns
  /// clockwise on screen
  fn limb(hips: BoneSetup, goal: Vec2, setup: IkSetup) -> SkeletonDataBuilder {
    limb_bones(hips, goal).ik("reach", &["upper", "lower"], "goal", setup)
  }

  fn built(builder: SkeletonDataBuilder) -> Rig {
    Rig::new(builder.build().expect("a valid skeleton"))
  }

  /// A bone's transform in the skeleton's space, as the pose has it
  fn space(skeleton: &Skeleton, name: &str) -> Affine2 {
    let bone = skeleton.bone_id(name).expect("a bone of the skeleton");
    skeleton.pose.skeleton_space[bone.index()]
  }

  /// A bone's own values, as the pose has them
  fn own(skeleton: &Skeleton, name: &str) -> BonePose {
    let bone = skeleton.bone_id(name).expect("a bone of the skeleton");
    skeleton.pose.bones[bone.index()]
  }

  /// Where a bone is in the world
  fn world(skeleton: &Skeleton, name: &str) -> Vec2 {
    let world = skeleton.bone_world(name).expect("a posed bone");
    world.translation
  }

  /// The angle of a bone's x axis in the world
  fn heading(skeleton: &Skeleton, name: &str) -> f32 {
    let world = skeleton.bone_world(name).expect("a posed bone");
    world.matrix2.x_axis.to_angle()
  }

  fn near(actual: Vec2, expected: Vec2) {
    assert!(
      (actual - expected).length() < 0.01,
      "expected {expected}, got {actual}"
    );
  }

  /// Which side of a limb its joint is on: positive when the way from
  /// `origin` to `joint` and on to `tip` turns the positive way
  fn side(origin: Vec2, joint: Vec2, tip: Vec2) -> f32 {
    (joint - origin).perp_dot(tip - joint)
  }

  /// Transforms to the bit
  fn bits(transforms: &[Affine2]) -> Vec<[u32; 6]> {
    let bits = |transform: &Affine2| transform.to_cols_array().map(f32::to_bits);
    transforms.iter().map(bits).collect()
  }

  /// Bones' own values to the bit
  fn own_bits(bones: &[BonePose]) -> Vec<[u32; 7]> {
    let values = |bone: &BonePose| {
      [
        bone.x,
        bone.y,
        bone.rotation,
        bone.scale_x,
        bone.scale_y,
        bone.shear_x,
        bone.shear_y,
      ]
    };
    bones
      .iter()
      .map(|bone| values(bone).map(f32::to_bits))
      .collect()
  }

  /// The room a skeleton's constraint scratch has
  fn room(state: &Constrained) -> [(usize, usize); 2] {
    [
      (state.chain.as_ptr() as usize, state.chain.capacity()),
      (state.joints.as_ptr() as usize, state.joints.capacity()),
    ]
  }

  #[test]
  fn a_one_bone_two_bone_and_chain_constraint_each_reach_their_target_in_a_step() {
    // Three limbs on root bones of their own, each with its target, on an
    // entity standing at (100, 50): a place in the world is the place in
    // the skeleton's space plus that
    let link = |turn: f32| BoneSetup {
      x: 10.0,
      length: 10.0,
      rotation: turn,
      ..BoneSetup::default()
    };
    let stretched = IkSetup {
      stretch: true,
      uniform: true,
      ..IkSetup::default()
    };
    let rig = built(
      SkeletonData::builder()
        // One bone 5 long at (10, 0), with a mark at its tip, and its
        // target 20 below its origin
        .bone("eye", None, long(10.0, 5.0))
        .bone("pupil", Some("eye"), at(5.0, 0.0))
        .bone("look", None, at(10.0, 20.0))
        // Two bones, 40 and 30 long, from (20, 0), with a mark at their
        // tip, and their target 50 below their origin
        .bone("upper", None, long(20.0, 40.0))
        .bone("lower", Some("upper"), long(40.0, 30.0))
        .bone("hand", Some("lower"), at(30.0, 0.0))
        .bone("goal", None, at(20.0, 50.0))
        // A chain of four bones from (-50, 0), each 10 long and turned 0.3
        // from the one before, with a mark at its tip; its target is 26.4
        // from its first joint, two thirds of its length of 40
        .bone(
          "tail",
          None,
          BoneSetup {
            x: -50.0,
            ..link(0.3)
          },
        )
        .bone("tail2", Some("tail"), link(0.3))
        .bone("tail3", Some("tail2"), link(0.3))
        .bone("tail4", Some("tail3"), link(0.3))
        .bone("end", Some("tail4"), at(10.0, 0.0))
        .bone("bait", None, at(-34.0, 21.0))
        .ik("gaze", &["eye"], "look", stretched)
        .ik("reach", &["upper", "lower"], "goal", IkSetup::default())
        .ik(
          "swish",
          &["tail", "tail2", "tail3", "tail4"],
          "bait",
          IkSetup::default(),
        ),
    );
    let stood = Vec2::new(100.0, 50.0);
    let mut skeleton = rig.posed(Affine2::from_translation(stood), |_| {});

    // One bone points straight down at its target, a quarter turn, and
    // with stretch is scaled 4 times (a target 20 away over a length of 5)
    // so that its tip is on it: the mark 5 along it is at (10, 20) in the
    // skeleton's space. `uniform` is the setup's: the bone is as much
    // wider, its y axis 4 long and pointing left
    near(world(&skeleton, "pupil"), stood + Vec2::new(10.0, 20.0));
    near(space(&skeleton, "eye").matrix2.y_axis, Vec2::new(-4.0, 0.0));

    // Two bones: the joint is 24 right of and 32 below the limb's origin
    // at (20, 0), and the tip is on the target
    near(world(&skeleton, "lower"), stood + Vec2::new(44.0, 32.0));
    near(world(&skeleton, "hand"), stood + Vec2::new(20.0, 50.0));

    // A chain's tip is on its target, as near as a chain's solve promises
    // (0.001 of its length) and 0.01 over
    let end = world(&skeleton, "end");
    let bait = stood + Vec2::new(-34.0, 21.0);
    assert!(end.distance(bait) < CHAIN_NEAR * 40.0 + 0.01, "{end}");

    // A steady step takes no more room: the chain is solved in scratch
    // the skeleton keeps
    let kept = room(&skeleton.constrained);
    for _ in 0..3 {
      rig.step(&mut skeleton, Affine2::from_translation(stood));
    }
    assert_eq!(room(&skeleton.constrained), kept);
    near(world(&skeleton, "end"), end);

    // And that scratch is sized as the data is taken up, for its longest
    // chain: the pass itself takes no room
    let data = rig.data();
    let mut state = Constrained::new(data);
    assert!(state.chain.capacity() >= 4);
    let sized = room(&state);
    let mut pose = Pose::setup(data);
    pose.compute_world(data, Affine2::IDENTITY);
    let tip = data.bone("end").expect("the mark").index();
    let unsolved = pose.skeleton_space[tip].translation;
    constrain(&mut pose, data, Affine2::IDENTITY, &[], &mut state);
    assert_eq!(room(&state), sized);
    assert!(pose.skeleton_space[tip].translation.distance(unsolved) > 1.0);
    near(pose.skeleton_space[tip].translation, end - stood);
  }

  #[test]
  fn constraints_apply_in_their_order() {
    // One bone at the origin and two targets, one 10 below it and one 10
    // to its left, with a constraint for each at a full mix: the bone ends
    // up pointing at the target of whichever constraint comes later
    let eye = |first: &str, second: &str| {
      let rig = built(
        SkeletonData::builder()
          .bone("eye", None, long(0.0, 10.0))
          .bone("below", None, at(0.0, 10.0))
          .bone("left", None, at(-10.0, 0.0))
          .ik(format!("to {first}"), &["eye"], first, IkSetup::default())
          .ik(format!("to {second}"), &["eye"], second, IkSetup::default()),
      );
      let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
      space(&skeleton, "eye").matrix2.x_axis
    };
    near(eye("below", "left"), Vec2::new(-1.0, 0.0));
    near(eye("left", "below"), Vec2::new(0.0, 1.0));

    // An arm 20 long at the origin carries a mark at its tip, (20, 0) as
    // animated; the arm's target is 30 below the origin. An eye at (40, 0)
    // looks at the mark
    let looking = |arm_first: bool| {
      let bones = SkeletonData::builder()
        .bone("arm", None, long(0.0, 20.0))
        .bone("mark", Some("arm"), at(20.0, 0.0))
        .bone("goal", None, at(0.0, 30.0))
        .bone("eye", None, long(40.0, 10.0));
      let rig = built(if arm_first {
        bones.ik("reach", &["arm"], "goal", IkSetup::default()).ik(
          "gaze",
          &["eye"],
          "mark",
          IkSetup::default(),
        )
      } else {
        bones.ik("gaze", &["eye"], "mark", IkSetup::default()).ik(
          "reach",
          &["arm"],
          "goal",
          IkSetup::default(),
        )
      });
      let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
      // Either way the arm ends up pointing down, its mark at (0, 20)
      near(space(&skeleton, "mark").translation, Vec2::new(0.0, 20.0));
      space(&skeleton, "eye").matrix2.x_axis
    };
    // The arm first: the eye looks at the mark where the arm has put it,
    // from (40, 0) toward (0, 20)
    near(looking(true), Vec2::new(-40.0, 20.0).normalize());
    // The eye first: it looks at the mark where animation left it,
    // straight to its left, and the arm's turn afterwards does not turn it
    near(looking(false), Vec2::new(-1.0, 0.0));
  }

  #[test]
  fn a_constraint_places_the_descendants_of_its_bones_again() {
    // An arm 20 long at the origin, with a hand at its tip, a finger 5
    // along the hand, and a watch on its side at (10, 3). Its target is 30
    // below the origin, so the arm is turned a quarter turn to point down
    // the screen: (x, y) in the arm's space is then (-y, x) in the
    // skeleton's. A staff stands apart at (50, 0). The bones are listed so
    // that those below the arm are not next to one another
    let rig = built(
      SkeletonData::builder()
        .bone("root", None, BoneSetup::default())
        .bone("arm", Some("root"), long(0.0, 20.0))
        .bone("staff", Some("root"), at(50.0, 0.0))
        .bone("hand", Some("arm"), long(20.0, 5.0))
        .bone("goal", Some("root"), at(0.0, 30.0))
        .bone("finger", Some("hand"), at(5.0, 0.0))
        .bone("watch", Some("arm"), at(10.0, 3.0))
        .ik("reach", &["arm"], "goal", IkSetup::default())
        .transform_constraint("carry", &["hand"], "staff", TransformSetup::default()),
    );
    let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
    // The hand, the finger and the watch follow the arm in the same step
    let down = Vec2::new(0.0, 1.0);
    near(space(&skeleton, "arm").matrix2.x_axis, down);
    near(space(&skeleton, "hand").translation, Vec2::new(0.0, 20.0));
    near(space(&skeleton, "hand").matrix2.x_axis, down);
    near(space(&skeleton, "finger").translation, Vec2::new(0.0, 25.0));
    near(space(&skeleton, "watch").translation, Vec2::new(-3.0, 10.0));
    near(space(&skeleton, "watch").matrix2.x_axis, down);
    near(space(&skeleton, "staff").translation, Vec2::new(50.0, 0.0));

    // What the data worked out for the pass: after an IK constraint its
    // bones and every bone below them are placed again, in bone order;
    // after each bone of a transform constraint the bones below it; and
    // the arm is the one bone an IK constraint changes
    let lists = &rig.data().index().constraint_bones;
    let ids = |bones: &[u8]| bones.iter().map(|&bone| BoneId(bone)).collect::<Vec<_>>();
    assert_eq!(lists.again(0), ids(&[1, 3, 5, 6]));
    assert_eq!(lists.again(1), ids(&[5]));
    assert_eq!(lists.again(2), ids(&[]), "no such constraint");
    assert_eq!(lists.changed, ids(&[1]));
    assert!(lists.parts(0).is_empty(), "an IK constraint has no parts");
    let [(hand, after)] = lists.parts(1) else {
      panic!("one bone: {:?}", lists.parts(1));
    };
    assert_eq!((*hand, lists.listed(*after)), (BoneId(3), &ids(&[5])[..]));

    // A transform constraint on the arm and the hand, named hand first:
    // its bones in bone order, each with the bones below it; in a local
    // mode with itself first, and those bones are changed
    for local in [false, true] {
      let setup = TransformSetup {
        local,
        ..TransformSetup::default()
      };
      let data = SkeletonData::builder()
        .bone("root", None, BoneSetup::default())
        .bone("arm", Some("root"), long(0.0, 20.0))
        .bone("staff", Some("root"), at(50.0, 0.0))
        .bone("hand", Some("arm"), long(20.0, 5.0))
        .bone("finger", Some("hand"), at(5.0, 0.0))
        .bone("watch", Some("arm"), at(10.0, 3.0))
        .transform_constraint("carry", &["hand", "arm"], "staff", setup)
        .build()
        .expect("a valid skeleton");
      let lists = &data.index().constraint_bones;
      let parts: Vec<(BoneId, Vec<BoneId>)> = lists
        .parts(0)
        .iter()
        .map(|&(bone, after)| (bone, lists.listed(after).to_vec()))
        .collect();
      let (arm, hand) = if local {
        (ids(&[1, 3, 4, 5]), ids(&[3, 4]))
      } else {
        (ids(&[3, 4, 5]), ids(&[4]))
      };
      assert_eq!(parts, [(BoneId(1), arm), (BoneId(3), hand)]);
      let changed = if local { ids(&[1, 3]) } else { ids(&[]) };
      assert_eq!(lists.changed, changed);
    }
  }

  /// Puts the limb's target at a point in the world
  fn put_goal(skeleton: &mut Skeleton, at: Vec2) {
    let put = BoneOverride {
      translation: Some(at),
      space: Space::World,
      ..BoneOverride::default()
    };
    skeleton.override_bone("goal", put);
  }

  #[test]
  fn a_target_moved_by_a_world_override_is_reached_in_the_same_step() {
    // The limb lies straight along x on an entity at (100, 50) drawn at
    // twice its size, its target at its tip. The game puts the target at a
    // point in the world, 100 below the entity: (0, 50) in the skeleton's
    // space, so the joint is at (24, 32) there
    let rig = built(limb(
      BoneSetup::default(),
      Vec2::new(70.0, 0.0),
      IkSetup::default(),
    ));
    let root = Affine2::from_scale_angle_translation(Vec2::splat(2.0), 0.0, Vec2::new(100.0, 50.0));
    let mut skeleton = rig.skeleton();
    put_goal(&mut skeleton, Vec2::new(100.0, 150.0));
    rig.step(&mut skeleton, root);
    near(world(&skeleton, "hand"), Vec2::new(100.0, 150.0));
    near(world(&skeleton, "lower"), Vec2::new(148.0, 114.0));

    // Moved again, to (30, 40) of the skeleton, it is reached in the one
    // step that follows: the joint is at (38.4, 11.2) of the skeleton
    put_goal(&mut skeleton, Vec2::new(160.0, 130.0));
    rig.step(&mut skeleton, root);
    near(world(&skeleton, "hand"), Vec2::new(160.0, 130.0));
    near(world(&skeleton, "lower"), Vec2::new(176.8, 72.4));
  }

  #[test]
  fn a_world_override_below_an_ik_bone_still_holds() {
    // The limb as a leg, its "hand" the foot, on an entity turned 0.4 and
    // standing at (100, 50). The game holds the foot at an angle of 0.25
    // in the world, and moves the leg's target about the skeleton's space
    let rig = built(limb(
      BoneSetup::default(),
      Vec2::new(70.0, 0.0),
      IkSetup::default(),
    ));
    let root = Affine2::from_angle_translation(0.4, Vec2::new(100.0, 50.0));
    let held = BoneOverride {
      rotation: Some(0.25),
      space: Space::World,
      ..BoneOverride::default()
    };
    let move_goal = |skeleton: &mut Skeleton, to: Vec2| {
      let moved = BoneOverride {
        translation: Some(to),
        ..BoneOverride::default()
      };
      skeleton.override_bone("goal", moved);
    };
    // Each target, and the way the shin then points in the skeleton's
    // space: from the joint to the target, (-24, 18) and (-8.4, 28.8)
    let targets = [
      (Vec2::new(0.0, 50.0), Vec2::new(-0.8, 0.6)),
      (Vec2::new(30.0, 40.0), Vec2::new(-0.28, 0.96)),
    ];
    let mut skeleton = rig.skeleton();
    skeleton.override_bone("hand", held);
    for (goal, shin) in targets {
      move_goal(&mut skeleton, goal);
      rig.step(&mut skeleton, root);
      // The leg has reached, the shin turned from where it lay
      near(space(&skeleton, "hand").translation, goal);
      near(space(&skeleton, "lower").matrix2.x_axis, shin);
      // And the foot points where the game asked, whatever the shin did
      let off = shortest(heading(&skeleton, "hand") - 0.25);
      assert!(off.abs() < 0.001, "target {goal}: off by {off}");
    }

    // At half its mix the override is applied from the foot's animated
    // values, not from what it made of them before the leg was solved:
    // the foot ends half way, the short way, from where the shin carries
    // it (it has no turn of its own) to the angle asked for
    skeleton.override_bone("hand", BoneOverride { mix: 0.5, ..held });
    for (goal, _) in targets {
      move_goal(&mut skeleton, goal);
      rig.step(&mut skeleton, root);
      let carried = heading(&skeleton, "lower");
      let half_way = carried + shortest(0.25 - carried) * 0.5;
      let off = shortest(heading(&skeleton, "hand") - half_way);
      assert!(off.abs() < 0.001, "target {goal}: off by {off}");
    }
  }

  #[test]
  fn an_override_first_set_between_steps_holds_when_its_skeleton_is_placed_again() {
    // The leg of the test above, stepped once with its target at (0, 50),
    // and then the foot held at 0.25 in the world before the next step.
    // Placed again (it rides a socket, or is flipped), the override takes
    // its animated values from the pose as it stands, so it is applied
    // afresh below the constraint: the foot points where the game asked,
    // and not 2.498 off, by what the shin turned
    let rig = built(limb(
      BoneSetup::default(),
      Vec2::new(0.0, 50.0),
      IkSetup::default(),
    ));
    let root = Affine2::from_angle_translation(0.4, Vec2::new(100.0, 50.0));
    let held = BoneOverride {
      rotation: Some(0.25),
      space: Space::World,
      ..BoneOverride::default()
    };
    let mut skeleton = rig.posed(root, |_| {});
    skeleton.override_bone("hand", held);
    for again in 1..=2 {
      skeleton.place_again(rig.data(), root.into());
      let off = shortest(heading(&skeleton, "hand") - 0.25);
      assert!(
        off.abs() < 0.001,
        "placed again {again} times: off by {off}"
      );
    }
  }

  #[test]
  fn an_override_at_a_part_mix_first_set_between_steps_is_applied_once_however_often_it_is_placed_again()
   {
    // An override at half its mix on the limb's hips, which no constraint
    // changes, set after a step: it is applied once, from what the step
    // left, however often the skeleton is placed again before its next
    // step. The hips turn half way to 1, and not three quarters of the way
    // the second time
    let rig = built(limb(
      BoneSetup::default(),
      Vec2::new(0.0, 50.0),
      IkSetup::default(),
    ));
    let mut skeleton = rig.posed(Affine2::IDENTITY, |_| {});
    let half = BoneOverride {
      rotation: Some(1.0),
      mix: 0.5,
      ..BoneOverride::default()
    };
    skeleton.override_bone("hips", half);
    for again in 1..=2 {
      skeleton.place_again(rig.data(), Affine2::IDENTITY.into());
      let turned = own(&skeleton, "hips").rotation;
      assert!(
        (turned - 0.5).abs() < 0.001,
        "placed again {again} times: {turned}"
      );
    }
  }

  #[test]
  fn a_limb_under_a_moved_turned_and_unevenly_scaled_parent_reaches_its_target() {
    // The limb's parent "hips" stands at (20, -10) of the skeleton, turned
    // 0.5 and scaled (1.5, 0.7). The limb's own bones are even, so the
    // solve is exact in the hips' space. Its target is a bone under the
    // hips at (30, 40) there, 50 from the limb's origin and within its
    // reach of 70: the 3-4-5 triangle puts the joint at (38.4, 11.2) in
    // the hips' space
    let hips = BoneSetup {
      x: 20.0,
      y: -10.0,
      rotation: 0.5,
      scale_x: 1.5,
      scale_y: 0.7,
      ..BoneSetup::default()
    };
    let rig = built(
      SkeletonData::builder()
        .bone("hips", None, hips)
        .bone("upper", Some("hips"), long(0.0, 40.0))
        .bone("lower", Some("upper"), long(40.0, 30.0))
        .bone("hand", Some("lower"), at(30.0, 0.0))
        .bone("goal", Some("hips"), at(30.0, 40.0))
        .ik("reach", &["upper", "lower"], "goal", IkSetup::default()),
    );
    let root = Affine2::from_angle_translation(-0.3, Vec2::new(100.0, 50.0));
    let skeleton = rig.posed(root, |_| {});
    near(world(&skeleton, "hand"), world(&skeleton, "goal"));
    let back = space(&skeleton, "hips").inverse();
    let in_hips = |name: &str| back.transform_point2(space(&skeleton, name).translation);
    near(in_hips("hand"), Vec2::new(30.0, 40.0));
    near(in_hips("lower"), Vec2::new(38.4, 11.2));
  }

  #[test]
  fn world_and_local_overrides_below_a_limb_come_out_as_asked_after_it_moves() {
    // The limb, its target at (0, 50), with a watch on the shin at
    // (10, 3), on an entity turned 0.4 at (100, 50). The game puts the
    // hand at a point in the world, and turns and moves the watch half way
    // to values of its own
    let rig = built(
      limb_bones(BoneSetup::default(), Vec2::new(0.0, 50.0))
        .bone("watch", Some("lower"), at(10.0, 3.0))
        .ik("reach", &["upper", "lower"], "goal", IkSetup::default()),
    );
    let root = Affine2::from_angle_translation(0.4, Vec2::new(100.0, 50.0));
    let point = root.transform_point2(Vec2::new(10.0, 60.0));
    let skeleton = rig.posed(root, |skeleton| {
      let put = BoneOverride {
        translation: Some(point),
        space: Space::World,
        ..BoneOverride::default()
      };
      skeleton.override_bone("hand", put);
      let set = BoneOverride {
        rotation: Some(0.7),
        translation: Some(Vec2::new(12.0, -4.0)),
        mix: 0.5,
        ..BoneOverride::default()
      };
      skeleton.override_bone("watch", set);
    });
    // The limb has moved: the shin points from the joint to the target
    near(
      space(&skeleton, "lower").matrix2.x_axis,
      Vec2::new(-0.8, 0.6),
    );
    // The hand is at the point asked for
    near(world(&skeleton, "hand"), point);
    // The watch has its own values half way from the animated ones, once,
    // and is placed by them under the shin as the limb left it
    let watch = own(&skeleton, "watch");
    assert!((watch.rotation - 0.35).abs() < 0.001, "{}", watch.rotation);
    near(Vec2::new(watch.x, watch.y), Vec2::new(11.0, -0.5));
    let placed = space(&skeleton, "lower") * watch.affine();
    near(space(&skeleton, "watch").translation, placed.translation);
    near(
      space(&skeleton, "watch").matrix2.x_axis,
      placed.matrix2.x_axis,
    );
  }

  #[test]
  fn a_constraint_has_the_last_word_over_an_override_on_its_own_bone() {
    // An eye at the origin whose target is 10 below it, and a game that
    // turns the eye to 2 radians: the constraint points it at the target,
    // a quarter turn
    let eye = |mix: f32| {
      let setup = IkSetup {
        mix,
        ..IkSetup::default()
      };
      built(
        SkeletonData::builder()
          .bone("eye", None, long(0.0, 10.0))
          .bone("look", None, at(0.0, 10.0))
          .ik("gaze", &["eye"], "look", setup),
      )
    };
    let turned = |rotation: f32| BoneOverride {
      rotation: Some(rotation),
      ..BoneOverride::default()
    };
    let rig = eye(1.0);
    let skeleton = rig.posed(Affine2::IDENTITY, |skeleton| {
      skeleton.override_bone("eye", turned(2.0));
    });
    near(space(&skeleton, "eye").matrix2.x_axis, Vec2::new(0.0, 1.0));

    // The constraint starts from the bone as the override left it: at
    // half its mix the eye is half way from the override's 0.5 to the
    // quarter turn, not half way from its animated 0
    let rig = eye(0.5);
    let skeleton = rig.posed(Affine2::IDENTITY, |skeleton| {
      skeleton.override_bone("eye", turned(0.5));
    });
    let half_way = 0.5 + (FRAC_PI_2 - 0.5) * 0.5;
    assert!((own(&skeleton, "eye").rotation - half_way).abs() < 0.001);

    // The eye on a hip, held at 0.3 in the world: "gaze" turns it, and
    // when "lean" then turns the hip the eye is placed again. The eye is
    // the constraint's by then: it keeps the quarter turn of its own, so
    // it is carried round to point left, from (0, 10), and is not put back
    // at the game's angle
    let skeleton = eye_on_a_hip(1.0).posed(Affine2::IDENTITY, hold_the_eye);
    assert!((own(&skeleton, "eye").rotation - FRAC_PI_2).abs() < 0.001);
    near(space(&skeleton, "eye").translation, Vec2::new(0.0, 10.0));
    near(space(&skeleton, "eye").matrix2.x_axis, Vec2::new(-1.0, 0.0));

    // A constraint at a mix of 0 is passed over, as one under a flat
    // parent is: it changes nothing and marks nothing, so the eye is left
    // to the game. When "lean" turns the hip the eye's override is applied
    // afresh, and the eye points at the game's 0.3 in the world, from
    // where the hip carries it
    let skeleton = eye_on_a_hip(0.0).posed(Affine2::IDENTITY, hold_the_eye);
    let off = shortest(heading(&skeleton, "eye") - 0.3);
    assert!(off.abs() < 0.001, "off by {off}");
    near(space(&skeleton, "eye").translation, Vec2::new(0.0, 10.0));
  }

  /// An eye 10 along a hip at the origin. "gaze", at `gaze` for its mix,
  /// points the eye at a target 20 below it, a quarter turn in the hip's
  /// space; "lean", listed after it, turns the hip a quarter turn toward a
  /// target 10 below it
  fn eye_on_a_hip(gaze: f32) -> Rig {
    let gaze = IkSetup {
      mix: gaze,
      ..IkSetup::default()
    };
    built(
      SkeletonData::builder()
        .bone("hip", None, long(0.0, 10.0))
        .bone("eye", Some("hip"), long(10.0, 5.0))
        .bone("look", None, at(10.0, 20.0))
        .bone("aim", None, at(0.0, 10.0))
        .ik("gaze", &["eye"], "look", gaze)
        .ik("lean", &["hip"], "aim", IkSetup::default()),
    )
  }

  /// What a game sets on [`eye_on_a_hip`]: the eye held at 0.3 in the
  /// world
  fn hold_the_eye(skeleton: &mut Skeleton) {
    let held = BoneOverride {
      rotation: Some(0.3),
      space: Space::World,
      ..BoneOverride::default()
    };
    skeleton.override_bone("eye", held);
  }

  #[test]
  fn a_world_override_on_an_ik_bone_is_the_game_s_at_a_mix_of_0_and_the_constraint_s_above_it() {
    // On an entity turned 0.4 and standing at (100, 50), so that the world
    // and the skeleton's space differ. Limit: the eye's treatment changes
    // as the mix of "gaze" leaves 0. At 0 the constraint is passed over
    // and the eye's override is applied afresh below the turned hip: the
    // eye points at 0.3 in the world
    let root = Affine2::from_angle_translation(0.4, Vec2::new(100.0, 50.0));
    let skeleton = eye_on_a_hip(0.0).posed(root, hold_the_eye);
    let off = shortest(heading(&skeleton, "eye") - 0.3);
    assert!(off.abs() < 0.001, "at 0: off by {off}");
    near(
      world(&skeleton, "eye"),
      root.transform_point2(Vec2::new(0.0, 10.0)),
    );

    // At a mix of 0.01 the constraint has the last word: the eye keeps its
    // own rotation, the override's (0.3 in the world less the entity's
    // 0.4, under the hip as it first lay) turned a hundredth of the way to
    // the quarter turn, and the hip's quarter turn and the entity's 0.4
    // carry that round, about 1.6 from where a mix of 0 leaves it
    let skeleton = eye_on_a_hip(0.01).posed(root, hold_the_eye);
    let held = 0.3 - 0.4;
    let kept = held + (FRAC_PI_2 - held) * 0.01;
    let off = shortest(heading(&skeleton, "eye") - (0.4 + kept + FRAC_PI_2));
    assert!(off.abs() < 0.001, "at 0.01: off by {off}");

    // A mix that is not a number is passed over as 0 is
    let rig = eye_on_a_hip(0.0);
    let mut skeleton = rig.posed(root, hold_the_eye);
    skeleton.pose.constraints[0] = ConstraintValues::Ik(IkValues {
      mix: f32::NAN,
      ..IkValues::of(&IkSetup::default())
    });
    skeleton.place_again(rig.data(), root.into());
    let off = shortest(heading(&skeleton, "eye") - 0.3);
    assert!(off.abs() < 0.001, "not a number: off by {off}");
  }

  #[test]
  fn a_flat_parent_leaves_the_bones_as_animated() {
    // An arm, a hand and a finger under a parent bone, with a constraint
    // of each size on them, and a target apart
    let under = |scale_x: f32| {
      let parent = BoneSetup {
        scale_x,
        ..BoneSetup::default()
      };
      let part = |x: f32, rotation: f32| BoneSetup {
        x,
        rotation,
        length: 10.0,
        ..BoneSetup::default()
      };
      built(
        SkeletonData::builder()
          .bone("parent", None, parent)
          .bone("arm", Some("parent"), part(5.0, 0.3))
          .bone("hand", Some("arm"), part(10.0, 0.2))
          .bone("finger", Some("hand"), part(10.0, 0.1))
          .bone("goal", None, at(3.0, 14.0))
          .ik("point", &["arm"], "goal", IkSetup::default())
          .ik("reach", &["arm", "hand"], "goal", IkSetup::default())
          .ik(
            "curl",
            &["arm", "hand", "finger"],
            "goal",
            IkSetup::default(),
          ),
      )
    };
    // Under a whole parent the constraints turn the bones
    let rig = under(1.0);
    let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
    assert_ne!(own(&skeleton, "arm").rotation, 0.3);

    // Under a parent scaled to nothing along x, or to so little that no
    // point can be brought back through it, no target can be brought into
    // the arm's parent's space: every bone keeps its animated values to
    // the bit, and nothing is not a number
    for flat in [0.0, 1e-20] {
      let rig = under(flat);
      let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
      let animated = Pose::setup(rig.data()).bones;
      assert_eq!(
        own_bits(&skeleton.pose.bones),
        own_bits(&animated),
        "a parent scaled {flat}"
      );
      let pose = &skeleton.pose;
      assert!(pose.skeleton_space.iter().all(|space| space.is_finite()));
      assert!(pose.world.iter().all(|world| world.is_finite()));
    }
  }

  #[test]
  fn a_pose_of_other_data_is_left_alone() {
    // An eye, its mark and its target, as the first three bones of a rig
    // of more; and a pose made from data that has those three bones alone,
    // with a constraint of its own. The rig's first constraint would find
    // every bone it names in that pose
    let eye = || {
      SkeletonData::builder()
        .bone("eye", None, long(0.0, 5.0))
        .bone("pupil", Some("eye"), at(5.0, 0.0))
        .bone("look", None, at(0.0, 20.0))
        .ik("gaze", &["eye"], "look", IkSetup::default())
    };
    let rig = built(eye().bone("brow", Some("eye"), at(0.0, -5.0)));
    let other = eye().build().expect("a valid skeleton");
    let mut pose = Pose::setup(&other);
    pose.compute_world(&other, Affine2::IDENTITY);
    let placed = (bits(&pose.skeleton_space), own_bits(&pose.bones));
    let data = rig.data();
    let mut state = Constrained::new(data);
    constrain(&mut pose, data, Affine2::IDENTITY, &[], &mut state);
    assert_eq!((bits(&pose.skeleton_space), own_bits(&pose.bones)), placed);
    // The data's own pose is constrained: the eye turns to look down
    let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
    near(space(&skeleton, "pupil").translation, Vec2::new(0.0, 5.0));
  }

  #[test]
  fn a_flipped_entity_and_a_mirrored_ancestor_keep_the_bend_side() {
    // The limb on an entity at (100, 50), its target at (30, 40) in the
    // skeleton's space: the joint is at (38.4, 11.2) there, on the
    // positive side of the limb
    let root = Affine2::from_translation(Vec2::new(100.0, 50.0));
    let flipped = Stand {
      root,
      flip: Flip { x: true, y: false },
    };
    let limb_to = |goal: Vec2| limb(BoneSetup::default(), goal, IkSetup::default());
    let sides = |skeleton: &Skeleton| {
      let places = |of: &dyn Fn(&str) -> Vec2| side(of("upper"), of("lower"), of("hand"));
      let in_space = places(&|name| space(skeleton, name).translation);
      let on_screen = places(&|name| world(skeleton, name));
      (in_space, on_screen)
    };
    let rig = built(limb_to(Vec2::new(30.0, 40.0)));
    let plain = rig.posed(root, |_| {});
    let mut mirrored = rig.skeleton();
    rig.step(&mut mirrored, flipped);
    near(space(&plain, "lower").translation, Vec2::new(38.4, 11.2));
    near(world(&plain, "lower"), Vec2::new(138.4, 61.2));
    near(world(&plain, "hand"), Vec2::new(130.0, 90.0));

    // A flipped entity: the pass works in the skeleton's own space, which
    // the flip does not reach, so the pose there is the same to the bit
    // and the joint is on the same side of the limb. In the world the limb
    // is drawn mirrored about the entity's x, joint and all
    assert_eq!(
      bits(&mirrored.pose.skeleton_space),
      bits(&plain.pose.skeleton_space)
    );
    near(world(&mirrored, "lower"), Vec2::new(61.6, 61.2));
    near(world(&mirrored, "hand"), Vec2::new(70.0, 90.0));
    let (in_space, on_screen) = sides(&plain);
    assert!(
      in_space > 1199.0 && on_screen > 1199.0,
      "{in_space}, {on_screen}"
    );
    let (in_space, on_screen) = sides(&mirrored);
    assert!(
      in_space > 1199.0 && on_screen < -1199.0,
      "{in_space}, {on_screen}"
    );

    // A target the game puts at a point in the world, (130, 90), is
    // reached flipped or not. Flipped, that point is (-30, 40) in the
    // skeleton's space: the joint is at (0, 40) there, still on the
    // positive side, and drawn at (100, 90), on the other side of the
    // limb as the screen shows it
    let rig = built(limb_to(Vec2::new(70.0, 0.0)));
    let aimed = Vec2::new(130.0, 90.0);
    let plain = rig.posed(root, |skeleton| put_goal(skeleton, aimed));
    let mut mirrored = rig.skeleton();
    put_goal(&mut mirrored, aimed);
    rig.step(&mut mirrored, flipped);
    near(world(&plain, "hand"), aimed);
    near(world(&plain, "lower"), Vec2::new(138.4, 61.2));
    near(world(&mirrored, "hand"), aimed);
    near(space(&mirrored, "lower").translation, Vec2::new(0.0, 40.0));
    near(world(&mirrored, "lower"), Vec2::new(100.0, 90.0));
    let (in_space, on_screen) = sides(&plain);
    assert!(
      in_space > 1199.0 && on_screen > 1199.0,
      "{in_space}, {on_screen}"
    );
    let (in_space, on_screen) = sides(&mirrored);
    assert!(
      in_space > 1199.0 && on_screen < -1199.0,
      "{in_space}, {on_screen}"
    );

    // A mirrored ancestor: the limb's parent is scaled (-1, 1), and the
    // target is at (30, 40) of the skeleton, which is (-30, 40) in the
    // parent's space. The bend is judged there: the joint is at (0, 40),
    // on the positive side of the limb in the parent's space, and so on
    // the negative side as the skeleton's space shows it. The limb is the
    // mirror image of the one a target at (-30, 40) gives under a parent
    // that is not mirrored
    let turned_round = BoneSetup {
      scale_x: -1.0,
      ..BoneSetup::default()
    };
    let goal = Vec2::new(30.0, 40.0);
    let rig = built(limb(turned_round, goal, IkSetup::default()));
    let skeleton = rig.posed(root, |_| {});
    near(space(&skeleton, "hand").translation, goal);
    near(space(&skeleton, "lower").translation, Vec2::new(0.0, 40.0));
    let back = space(&skeleton, "hips").inverse();
    let in_parent = |name: &str| back.transform_point2(space(&skeleton, name).translation);
    let judged = side(in_parent("upper"), in_parent("lower"), in_parent("hand"));
    let (in_space, _) = sides(&skeleton);
    assert!(
      judged > 1199.0 && in_space < -1199.0,
      "{judged}, {in_space}"
    );
    let unmirrored = built(limb_to(Vec2::new(-30.0, 40.0)));
    let twin = unmirrored.posed(root, |_| {});
    for name in ["lower", "hand"] {
      let image = space(&twin, name).translation * Vec2::new(-1.0, 1.0);
      near(space(&skeleton, name).translation, image);
    }
  }

  /// A hip 10 long at the origin that leans toward a target 30 below it,
  /// carrying the limb, which then reaches at half its mix for a target of
  /// its own; a badge on the shin that "pin" moves half way to the limb's
  /// target in the skeleton's space, and a patch on the hip that "sew"
  /// turns and moves by half the limb's first bone's own values; and an
  /// animation "lift" that turns the limb's first bone by 0.2
  fn leaning() -> Rig {
    let lift = Animation {
      name: Arc::from("lift"),
      duration: 1.0,
      timelines: vec![Timeline::Rotate {
        bone: BoneId(1),
        frames: vec![(0.0, 0.2, Curve::Linear)],
      }],
    };
    let half = IkSetup {
      mix: 0.5,
      ..IkSetup::default()
    };
    let pin = TransformSetup {
      mix: TransformMix {
        rotate: 0.5,
        x: 0.5,
        y: 0.5,
        scale_x: 0.5,
        scale_y: 0.5,
        shear_y: 0.5,
      },
      ..TransformSetup::default()
    };
    let sew = TransformSetup {
      mix: TransformMix {
        rotate: 0.5,
        x: 0.5,
        ..TransformMix::default()
      },
      local: true,
      relative: true,
      ..TransformSetup::default()
    };
    built(
      limb_bones(long(0.0, 10.0), Vec2::new(70.0, 0.0))
        .bone("aim", None, at(0.0, 30.0))
        .bone("badge", Some("lower"), at(10.0, 2.0))
        .bone("patch", Some("hips"), at(5.0, -2.0))
        .ik("lean", &["hips"], "aim", IkSetup::default())
        .ik("reach", &["upper", "lower"], "goal", half)
        .transform_constraint("pin", &["badge"], "goal", pin)
        .transform_constraint("sew", &["patch"], "upper", sew)
        .animation(lift),
    )
  }

  /// What a game sets on [`leaning`]: the limb's target at a point in the
  /// world, its second bone at an angle in the world, and the bone below
  /// the limb half way to another
  fn hold(skeleton: &mut Skeleton) {
    put_goal(skeleton, Vec2::new(130.0, 120.0));
    let aimed = |rotation: f32, mix: f32| BoneOverride {
      rotation: Some(rotation),
      space: Space::World,
      mix,
      ..BoneOverride::default()
    };
    skeleton.override_bone("lower", aimed(1.0, 1.0));
    skeleton.override_bone("hand", aimed(0.25, 0.5));
  }

  fn lifting() -> SkeletonAnimation {
    let mut animation = SkeletonAnimation::new();
    animation.set_animation(0, "lift", true);
    animation
  }

  #[test]
  fn a_skeleton_placed_again_has_its_step_s_pose() {
    let rig = leaning();
    let stand: Stand = Affine2::from_angle_translation(0.4, Vec2::new(100.0, 50.0)).into();
    let mut skeleton = rig.skeleton();
    hold(&mut skeleton);
    rig.step_playing(&mut skeleton, &mut lifting(), stand);
    // Every constraint has moved its bones: the hip leans a quarter turn,
    // the limb's first bone is not as the animation left it, the badge is
    // not where its own values put it under the shin, and the patch has
    // own values the constraint gave it
    assert!((own(&skeleton, "hips").rotation - FRAC_PI_2).abs() < 0.001);
    assert!((own(&skeleton, "upper").rotation - 0.2).abs() > 0.1);
    let badge = space(&skeleton, "lower") * own(&skeleton, "badge").affine();
    assert!(
      space(&skeleton, "badge")
        .translation
        .distance(badge.translation)
        > 1.0
    );
    assert!(own(&skeleton, "patch").rotation.abs() > 0.1);

    // Placed again where it stands, as a skeleton on a socket or a
    // flipped one is, once and then again: every bone is where the step
    // put it, to the bit, and has the own values the step gave it
    let stepped = (
      bits(&skeleton.pose.skeleton_space),
      bits(&skeleton.pose.world),
      own_bits(&skeleton.pose.bones),
    );
    for again in 1..=2 {
      skeleton.place_again(rig.data(), stand);
      let placed = (
        bits(&skeleton.pose.skeleton_space),
        bits(&skeleton.pose.world),
        own_bits(&skeleton.pose.bones),
      );
      assert_eq!(placed, stepped, "placed again {again} times");
    }

    // Placed again standing elsewhere, turned and flipped, it has the
    // pose a step there gives
    let elsewhere = Stand {
      root: Affine2::from_angle_translation(-0.2, Vec2::new(60.0, 80.0)),
      flip: Flip { x: true, y: false },
    };
    let mut twin = rig.skeleton();
    hold(&mut twin);
    rig.step_playing(&mut twin, &mut lifting(), elsewhere);
    for _ in 0..2 {
      skeleton.place_again(rig.data(), elsewhere);
      assert_eq!(
        bits(&skeleton.pose.skeleton_space),
        bits(&twin.pose.skeleton_space)
      );
      assert_eq!(bits(&skeleton.pose.world), bits(&twin.pose.world));
    }
    assert_ne!(bits(&twin.pose.skeleton_space), stepped.0);
  }

  #[test]
  fn a_skeleton_posed_in_the_frame_it_appears_has_its_constraints_applied() {
    // The limb lies straight, its target at its tip, (70, 0). An animation
    // keys the target to (0, 50) from its first moment
    let beckon = Animation {
      name: Arc::from("beckon"),
      duration: 1.0,
      timelines: vec![Timeline::Translate {
        bone: BoneId(4),
        frames: vec![(0.0, Vec2::new(-70.0, 50.0), Curve::Linear)],
      }],
    };
    let rig = built(
      limb(
        BoneSetup::default(),
        Vec2::new(70.0, 0.0),
        IkSetup::default(),
      )
      .animation(beckon),
    );
    let beckoning = || {
      let mut animation = SkeletonAnimation::new();
      animation.set_animation(0, "beckon", true);
      animation
    };
    let stood = Vec2::new(100.0, 50.0);
    let stand: Stand = Affine2::from_translation(stood).into();
    // Posed outside a step, where its animation stands: the limb has
    // reached the keyed target, its joint at (24, 32) of the skeleton
    let mut skeleton = rig.skeleton();
    let (data, seen) = (rig.data(), rig.seen());
    skeleton.appear(data, seen, Some(&mut beckoning()), stand);
    near(world(&skeleton, "hand"), stood + Vec2::new(0.0, 50.0));
    near(world(&skeleton, "lower"), stood + Vec2::new(24.0, 32.0));
    // It is drawn as it is, and it is the pose a step gives
    assert_eq!(skeleton.previous_space, skeleton.pose.skeleton_space);
    let mut twin = rig.skeleton();
    rig.step_playing(&mut twin, &mut beckoning(), stand);
    assert_eq!(
      bits(&skeleton.pose.skeleton_space),
      bits(&twin.pose.skeleton_space)
    );
  }

  #[test]
  fn the_pose_s_constraint_values_return_to_setup_each_step_without_allocating() {
    // The limb, its constraint with settings of its own, and a transform
    // constraint with mixes of its own
    let soft = IkSetup {
      mix: 0.75,
      softness: 4.0,
      bend_positive: false,
      stretch: true,
      ..IkSetup::default()
    };
    let level = TransformSetup {
      mix: TransformMix {
        rotate: 0.5,
        x: -1.0,
        ..TransformMix::default()
      },
      ..TransformSetup::default()
    };
    let rig = built(
      limb(BoneSetup::default(), Vec2::new(0.0, 50.0), soft)
        .transform_constraint("level", &["hand"], "hips", level)
        .animation(Animation {
          name: Arc::from("still"),
          duration: 1.0,
          timelines: Vec::new(),
        }),
    );
    // A pose starts with each constraint's setup values, in the data's
    // order
    let setup = [
      ConstraintValues::Ik(IkValues::of(&soft)),
      ConstraintValues::Transform(level.mix),
    ];
    assert_eq!(Pose::setup(rig.data()).constraints, setup);
    let mut skeleton = rig.posed(Affine2::IDENTITY, |_| {});
    assert_eq!(skeleton.pose.constraints, setup);
    assert_ne!(own(&skeleton, "upper").rotation, 0.0, "the limb reaches");

    // The pass applies the values the pose holds: with the limb's mix
    // there at 0, placing the bones again leaves the limb as animated
    let off = ConstraintValues::Ik(IkValues {
      mix: 0.0,
      ..IkValues::of(&soft)
    });
    let all = ConstraintValues::Transform(TransformMix::ALL);
    skeleton.pose.constraints[0] = off;
    skeleton.pose.constraints[1] = all;
    skeleton.place_again(rig.data(), Affine2::IDENTITY.into());
    assert_eq!(own(&skeleton, "upper").rotation, 0.0);
    // And with every mix of the transform constraint at 1 there, the hand
    // it names takes its target's place, the hips' at the origin
    near(space(&skeleton, "hand").translation, Vec2::ZERO);

    // A step puts every value back to its setup, in the list the pose
    // has: with no animation state, and with one
    let values = |skeleton: &Skeleton| {
      let values = &skeleton.pose.constraints;
      (values.as_ptr() as usize, values.capacity())
    };
    let kept = values(&skeleton);
    rig.step(&mut skeleton, Affine2::IDENTITY);
    assert_eq!(skeleton.pose.constraints, setup);
    assert_eq!(values(&skeleton), kept);
    assert_ne!(own(&skeleton, "upper").rotation, 0.0, "it reaches again");

    skeleton.pose.constraints[0] = off;
    skeleton.pose.constraints[1] = all;
    let mut animation = SkeletonAnimation::new();
    animation.set_animation(0, "still", true);
    rig.step_playing(&mut skeleton, &mut animation, Affine2::IDENTITY);
    assert_eq!(skeleton.pose.constraints, setup);
    assert_eq!(values(&skeleton), kept);
    assert_ne!(own(&skeleton, "upper").rotation, 0.0);
  }

  #[test]
  fn a_skeleton_without_constraints_keeps_no_constraint_state() {
    let plain = || {
      limb_bones(BoneSetup::default(), Vec2::new(0.0, 50.0))
        .build()
        .expect("a valid skeleton")
    };
    let nothing_kept = |skeleton: &Skeleton| {
      let state = &skeleton.constrained;
      assert!(state.animated.is_empty() && state.animated.capacity() == 0);
      assert_eq!(state.chain.capacity(), 0);
      assert_eq!(state.joints.capacity(), 0);
      assert_eq!(skeleton.pose.constraints.capacity(), 0);
    };
    // Nothing is kept, and nothing has room: the scratch is not allocated
    let mut rig = Rig::new(plain());
    let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
    nothing_kept(&skeleton);
    near(space(&skeleton, "hand").translation, Vec2::new(70.0, 0.0));
    let lists = &rig.data().index().constraint_bones;
    assert!(lists.again.is_empty() && lists.spans.is_empty() && lists.changed.is_empty());
    assert!(lists.parts.is_empty() && lists.part_spans.is_empty());

    // With a constraint on two bones, a skeleton keeps those bones'
    // animated values, and no scratch: only a chain is solved in one
    let reaching = limb(
      BoneSetup::default(),
      Vec2::new(0.0, 50.0),
      IkSetup::default(),
    );
    rig.reload(reaching.build().expect("a valid skeleton"));
    let mut skeleton = rig.posed(Affine2::IDENTITY, |_| {});
    let kept: Vec<BoneId> = skeleton
      .constrained
      .animated
      .iter()
      .map(|(bone, _)| *bone)
      .collect();
    assert_eq!(kept, [BoneId(1), BoneId(2)]);
    assert_eq!(room(&skeleton.constrained).map(|(_, room)| room), [0, 0]);
    assert_eq!(skeleton.pose.constraints.len(), 1);
    near(space(&skeleton, "hand").translation, Vec2::new(0.0, 50.0));

    // New data without constraints: what was kept goes with the old data
    rig.reload(plain());
    rig.step(&mut skeleton, Affine2::IDENTITY);
    nothing_kept(&skeleton);
    near(space(&skeleton, "hand").translation, Vec2::new(70.0, 0.0));
  }

  /// A copy at full mixes for place and rotation, in the mode `local` says
  fn copy_at(local: bool, mix: TransformMix) -> TransformSetup {
    TransformSetup {
      mix,
      local,
      ..TransformSetup::default()
    }
  }

  const PLACE_AND_TURN: TransformMix = TransformMix {
    rotate: 1.0,
    x: 1.0,
    y: 1.0,
    scale_x: 0.0,
    scale_y: 0.0,
    shear_y: 0.0,
  };

  /// A hip 10 long at the origin with a hand at its tip; a post, a root
  /// bone at (30, 5) turned 0.5, which "copy" makes the hand copy at full
  /// mixes for place and rotation, in the mode `local` says; and "lean",
  /// which turns the hip a quarter turn toward a target 10 below it.
  /// `copy_first` lists "copy" before "lean"
  fn copying(local: bool, copy_first: bool) -> Rig {
    let post = BoneSetup {
      rotation: 0.5,
      ..at(30.0, 5.0)
    };
    let bones = SkeletonData::builder()
      .bone("hip", None, long(0.0, 10.0))
      .bone("hand", Some("hip"), at(10.0, 0.0))
      .bone("post", None, post)
      .bone("aim", None, at(0.0, 10.0));
    let copy = |bones: SkeletonDataBuilder| {
      bones.transform_constraint("copy", &["hand"], "post", copy_at(local, PLACE_AND_TURN))
    };
    let lean = |bones: SkeletonDataBuilder| bones.ik("lean", &["hip"], "aim", IkSetup::default());
    built(if copy_first {
      lean(copy(bones))
    } else {
      copy(lean(bones))
    })
  }

  #[test]
  fn a_world_mode_result_is_lost_when_a_later_constraint_moves_an_ancestor() {
    // "lean" first: the hand is set where the post is, at (30, 5) and
    // turned 0.5 in the skeleton's space, and stays there
    let skeleton = copying(false, false).posed(Affine2::IDENTITY, |_| {});
    near(space(&skeleton, "hand").translation, Vec2::new(30.0, 5.0));
    near(
      space(&skeleton, "hand").matrix2.x_axis,
      Vec2::from_angle(0.5),
    );
    // Its own values are as they were: at the hip's tip, unturned
    let setup = Pose::setup(copying(false, false).data()).bones;
    assert_eq!(own_bits(&[own(&skeleton, "hand")]), own_bits(&setup[1..2]));

    // "copy" first: "lean" then turns the hip, and the hand is placed again
    // from its own values, at the tip of the hip pointing down, (0, 10):
    // the copy is lost
    let skeleton = copying(false, true).posed(Affine2::IDENTITY, |_| {});
    near(space(&skeleton, "hand").translation, Vec2::new(0.0, 10.0));
    near(space(&skeleton, "hand").matrix2.x_axis, Vec2::new(0.0, 1.0));

    // A world mode leaves the bone's own values, and so does not have the
    // last word on it: held at 0.2 in the world by the game, the hand
    // placed again has its override applied afresh, and points at 0.2,
    // not at 0.2 carried round a quarter turn by the hip
    let skeleton = copying(false, true).posed(Affine2::IDENTITY, |skeleton| {
      let held = BoneOverride {
        rotation: Some(0.2),
        space: Space::World,
        ..BoneOverride::default()
      };
      skeleton.override_bone("hand", held);
    });
    let off = shortest(heading(&skeleton, "hand") - 0.2);
    assert!(off.abs() < 0.001, "off by {off}");
  }

  #[test]
  fn an_own_values_result_is_kept_when_a_later_constraint_moves_an_ancestor() {
    // The hand's own values become the post's: at (30, 5) of the hip and
    // turned 0.5 there. When "lean" turns the hip a quarter turn, the hand
    // goes with it, to (-5, 30), pointing 0.5 past straight down; listed
    // either way round the result is the same. The game turns the hand to
    // -0.4 of its own: the constraint has the last word over that
    for copy_first in [true, false] {
      let skeleton = copying(true, copy_first).posed(Affine2::IDENTITY, |skeleton| {
        let turned = BoneOverride {
          rotation: Some(-0.4),
          ..BoneOverride::default()
        };
        skeleton.override_bone("hand", turned);
      });
      let hand = own(&skeleton, "hand");
      assert!((hand.rotation - 0.5).abs() < 0.001, "{}", hand.rotation);
      near(Vec2::new(hand.x, hand.y), Vec2::new(30.0, 5.0));
      near(space(&skeleton, "hand").translation, Vec2::new(-5.0, 30.0));
      let pointing = Vec2::from_angle(FRAC_PI_2 + 0.5);
      near(space(&skeleton, "hand").matrix2.x_axis, pointing);
    }
  }

  #[test]
  fn a_bone_and_its_descendant_in_one_constraint_are_both_constrained() {
    // An arm at the origin, a forearm 10 along it and a tip 5 along that,
    // all unturned; a post at (30, 5) turned 0.5. "follow" moves the arm
    // and the forearm (named forearm first) to the post's place, and half
    // way round to its turn, in the skeleton's space.
    //
    // The arm comes first: it goes to (30, 5), turned 0.25, and the forearm
    // is placed again under it, turned 0.25 too. The forearm is then read
    // where it now is, and turned half way from 0.25 to 0.5, to 0.375, at
    // (30, 5). The tip is placed again under it: 5 along 0.375 from there
    let post = BoneSetup {
      rotation: 0.5,
      ..at(30.0, 5.0)
    };
    let follow = TransformMix {
      rotate: 0.5,
      ..PLACE_AND_TURN
    };
    let rig = built(
      SkeletonData::builder()
        .bone("arm", None, long(0.0, 10.0))
        .bone("forearm", Some("arm"), long(10.0, 5.0))
        .bone("tip", Some("forearm"), at(5.0, 0.0))
        .bone("post", None, post)
        .transform_constraint(
          "follow",
          &["forearm", "arm"],
          "post",
          copy_at(false, follow),
        ),
    );
    let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
    let post = Vec2::new(30.0, 5.0);
    near(space(&skeleton, "arm").translation, post);
    near(
      space(&skeleton, "arm").matrix2.x_axis,
      Vec2::from_angle(0.25),
    );
    near(space(&skeleton, "forearm").translation, post);
    near(
      space(&skeleton, "forearm").matrix2.x_axis,
      Vec2::from_angle(0.375),
    );
    near(
      space(&skeleton, "tip").translation,
      post + 5.0 * Vec2::from_angle(0.375),
    );
  }

  #[test]
  fn a_world_mode_ignores_the_bone_s_inherit_mode() {
    // A parent at (5, 5), turned 0.7 and scaled 2, with a child 5 along it
    // in each inherit mode and a mark 5 along the child; a post at (30, 5)
    // turned 0.5. "copy" at every mix sets the child's transform in the
    // skeleton's space to the post's, whatever its mode, and leaves its
    // own values as they were; the mark is placed again under it
    let parent = BoneSetup {
      x: 5.0,
      y: 5.0,
      rotation: 0.7,
      scale_x: 2.0,
      scale_y: 2.0,
      ..BoneSetup::default()
    };
    let post = BoneSetup {
      rotation: 0.5,
      ..at(30.0, 5.0)
    };
    let modes = [
      Inherit::Normal,
      Inherit::OnlyTranslation,
      Inherit::NoRotationOrReflection,
      Inherit::NoScale,
      Inherit::NoScaleOrReflection,
    ];
    for inherit in modes {
      let child = BoneSetup {
        inherit,
        ..long(5.0, 5.0)
      };
      let rig = built(
        SkeletonData::builder()
          .bone("parent", None, parent)
          .bone("child", Some("parent"), child)
          .bone("mark", Some("child"), at(5.0, 0.0))
          .bone("post", None, post)
          .transform_constraint(
            "copy",
            &["child"],
            "post",
            copy_at(false, TransformMix::ALL),
          ),
      );
      let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
      let placed = space(&skeleton, "child");
      let (along, across) = (Vec2::from_angle(0.5), Vec2::from_angle(0.5 + FRAC_PI_2));
      near(placed.translation, Vec2::new(30.0, 5.0));
      near(placed.matrix2.x_axis, along);
      near(placed.matrix2.y_axis, across);
      let setup = Pose::setup(rig.data()).bones;
      assert_eq!(
        own_bits(&skeleton.pose.bones),
        own_bits(&setup),
        "{inherit:?}"
      );
      near(
        space(&skeleton, "mark").translation,
        Vec2::new(30.0, 5.0) + 5.0 * along,
      );
    }
  }

  #[test]
  fn an_own_values_mode_reads_the_target_as_animation_overrides_and_earlier_constraints_left_it() {
    // A post at the origin, keyed by "turn" to 0.4; a copier apart, whose
    // own rotation and x "copy" sets to the post's own; and, before or
    // after "copy", "point", which turns the post toward a target 10 below
    // the origin
    let turn = Animation {
      name: Arc::from("turn"),
      duration: 1.0,
      timelines: vec![Timeline::Rotate {
        bone: BoneId(0),
        frames: vec![(0.0, 0.4, Curve::Linear)],
      }],
    };
    let copy = TransformMix {
      rotate: 1.0,
      x: 1.0,
      ..TransformMix::default()
    };
    let rig = |point: Option<bool>| {
      let bones = SkeletonData::builder()
        .bone("post", None, long(0.0, 10.0))
        .bone("copier", None, at(40.0, 0.0))
        .bone("aim", None, at(0.0, 10.0))
        .animation(turn.clone());
      let copy = |bones: SkeletonDataBuilder| {
        bones.transform_constraint("copy", &["copier"], "post", copy_at(true, copy))
      };
      let point_at =
        |bones: SkeletonDataBuilder| bones.ik("point", &["post"], "aim", IkSetup::default());
      built(match point {
        None => copy(bones),
        Some(true) => copy(point_at(bones)),
        Some(false) => point_at(copy(bones)),
      })
    };
    let turning = || {
      let mut animation = SkeletonAnimation::new();
      animation.set_animation(0, "turn", true);
      animation
    };
    let moved = BoneOverride {
      rotation: Some(0.9),
      translation: Some(Vec2::new(12.0, 3.0)),
      ..BoneOverride::default()
    };
    let copied = |rig: &Rig, overridden: bool| {
      let mut skeleton = rig.skeleton();
      if overridden {
        skeleton.override_bone("post", moved);
      }
      rig.step_playing(&mut skeleton, &mut turning(), Affine2::IDENTITY);
      let copier = own(&skeleton, "copier");
      (copier.rotation, copier.x)
    };
    // As animation left it
    let (rotation, x) = copied(&rig(None), false);
    assert!(
      (rotation - 0.4).abs() < 0.001 && x.abs() < 0.001,
      "{rotation}, {x}"
    );
    // As the game's override left it
    let (rotation, x) = copied(&rig(None), true);
    assert!(
      (rotation - 0.9).abs() < 0.001 && (x - 12.0).abs() < 0.001,
      "{rotation}, {x}"
    );
    // As "point", listed before, left it: from (12, 3) toward (0, 10)
    let pointed = Vec2::new(-12.0, 7.0).to_angle();
    let (rotation, _) = copied(&rig(Some(true)), true);
    assert!(shortest(rotation - pointed).abs() < 0.001, "{rotation}");
    // Listed after, "point" turns the post once it has been read
    let (rotation, _) = copied(&rig(Some(false)), true);
    assert!((rotation - 0.9).abs() < 0.001, "{rotation}");
  }

  #[test]
  fn a_transform_constraint_then_an_ik_constraint_and_the_other_way_round() {
    // On one bone: an eye at the origin that "gaze" points at a target 10
    // below it, a quarter turn, and that "copy" turns to a post's 0.5, in
    // its own values. The later one's result stands
    let eye = |gaze_first: bool| {
      let post = BoneSetup {
        rotation: 0.5,
        ..BoneSetup::default()
      };
      let copy = TransformMix {
        rotate: 1.0,
        ..TransformMix::default()
      };
      let bones = SkeletonData::builder()
        .bone("eye", None, long(0.0, 10.0))
        .bone("look", None, at(0.0, 10.0))
        .bone("post", None, post);
      let gaze =
        |bones: SkeletonDataBuilder| bones.ik("gaze", &["eye"], "look", IkSetup::default());
      let copy = |bones: SkeletonDataBuilder| {
        bones.transform_constraint("copy", &["eye"], "post", copy_at(true, copy))
      };
      let rig = built(if gaze_first {
        copy(gaze(bones))
      } else {
        gaze(copy(bones))
      });
      let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
      own(&skeleton, "eye").rotation
    };
    assert!((eye(true) - 0.5).abs() < 0.001);
    assert!((eye(false) - FRAC_PI_2).abs() < 0.001);

    // A transform constraint whose target an IK constraint moves: an arm
    // that "reach" points down at a target, and a level apart whose turn
    // "match" sets to the arm's, in the skeleton's space. Arm first, the
    // level is turned down with it; level first, it copies the arm as
    // animation left it, unturned, and the arm turns afterwards
    let level = |reach_first: bool| {
      let bones = SkeletonData::builder()
        .bone("arm", None, long(0.0, 20.0))
        .bone("goal", None, at(0.0, 30.0))
        .bone("level", None, at(40.0, 0.0));
      let turn = TransformMix {
        rotate: 1.0,
        ..TransformMix::default()
      };
      let reach =
        |bones: SkeletonDataBuilder| bones.ik("reach", &["arm"], "goal", IkSetup::default());
      let copy = |bones: SkeletonDataBuilder| {
        bones.transform_constraint("match", &["level"], "arm", copy_at(false, turn))
      };
      let rig = built(if reach_first {
        copy(reach(bones))
      } else {
        reach(copy(bones))
      });
      let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
      near(space(&skeleton, "arm").matrix2.x_axis, Vec2::new(0.0, 1.0));
      space(&skeleton, "level").matrix2.x_axis
    };
    near(level(true), Vec2::new(0.0, 1.0));
    near(level(false), Vec2::new(1.0, 0.0));
  }

  #[test]
  fn a_transform_constraint_whose_mixes_are_all_0_is_passed_over() {
    // The eye on its hip, held at 0.3 in the world; "copy", listed before
    // "lean", would turn the eye's own values to a post's 1.0. At every
    // mix 0 it is passed over, and marks nothing: when "lean" turns the
    // hip the eye's override is applied afresh, and it points at 0.3
    let rig = |rotate: f32| {
      let post = BoneSetup {
        rotation: 1.0,
        ..at(40.0, 0.0)
      };
      let copy = TransformMix {
        rotate,
        ..TransformMix::default()
      };
      built(
        SkeletonData::builder()
          .bone("hip", None, long(0.0, 10.0))
          .bone("eye", Some("hip"), long(10.0, 5.0))
          .bone("post", None, post)
          .bone("aim", None, at(0.0, 10.0))
          .transform_constraint("copy", &["eye"], "post", copy_at(true, copy))
          .ik("lean", &["hip"], "aim", IkSetup::default()),
      )
    };
    let skeleton = rig(0.0).posed(Affine2::IDENTITY, hold_the_eye);
    let off = shortest(heading(&skeleton, "eye") - 0.3);
    assert!(off.abs() < 0.001, "at 0: off by {off}");

    // At a rotate mix of 0.01 it is applied and has the last word: the eye
    // keeps its own rotation, 0.3 moved a hundredth of the way to 1.0, and
    // the hip carries it round a quarter turn
    let skeleton = rig(0.01).posed(Affine2::IDENTITY, hold_the_eye);
    let kept = 0.3 + (1.0 - 0.3) * 0.01;
    let off = shortest(heading(&skeleton, "eye") - (kept + FRAC_PI_2));
    assert!(off.abs() < 0.001, "at 0.01: off by {off}");
  }

  #[test]
  fn a_pelvis_turns_six_tenths_of_its_trunk_s_lean() {
    // A hip turned 0.4 carries a torso, whose own rotation is a quarter
    // turn back (it points up the screen) plus a lean, and a pelvis, whose
    // own rotation is 0. "pelvis" copies six tenths of the torso's own
    // rotation plus a quarter turn: 0 + (-π/2 + lean + π/2 - 0) × 0.6
    for lean in [0.3, -0.2] {
      let hip = BoneSetup {
        rotation: 0.4,
        ..BoneSetup::default()
      };
      let torso = BoneSetup {
        rotation: -FRAC_PI_2 + lean,
        ..long(0.0, 20.0)
      };
      let follow = TransformSetup {
        mix: TransformMix {
          rotate: 0.6,
          ..TransformMix::default()
        },
        offset: TransformOffset {
          rotation: FRAC_PI_2,
          ..TransformOffset::default()
        },
        local: true,
        ..TransformSetup::default()
      };
      let rig = built(
        SkeletonData::builder()
          .bone("hip", None, hip)
          .bone("torso", Some("hip"), torso)
          .bone("pelvis", Some("hip"), long(0.0, 8.0))
          .transform_constraint("pelvis", &["pelvis"], "torso", follow),
      );
      let skeleton = rig.posed(Affine2::IDENTITY, |_| {});
      let turned = own(&skeleton, "pelvis").rotation;
      assert!((turned - 0.6 * lean).abs() < 0.001, "lean {lean}: {turned}");
      let off = shortest(heading(&skeleton, "pelvis") - (0.4 + 0.6 * lean));
      assert!(off.abs() < 0.001, "lean {lean}: off by {off}");
    }
  }

  #[test]
  fn an_override_first_set_between_steps_holds_on_a_skeleton_without_constraints() {
    // The limb's bones without its constraint, stepped, and then an
    // override at half its mix set on the hips: placed again twice, the
    // hips turn half way to 1 once, and not three quarters of the way
    let rig = built(limb_bones(BoneSetup::default(), Vec2::new(0.0, 50.0)));
    assert!(rig.data().constraints().is_empty());
    let mut skeleton = rig.posed(Affine2::IDENTITY, |_| {});
    let half = BoneOverride {
      rotation: Some(1.0),
      mix: 0.5,
      ..BoneOverride::default()
    };
    skeleton.override_bone("hips", half);
    for again in 1..=2 {
      skeleton.place_again(rig.data(), Affine2::IDENTITY.into());
      let turned = own(&skeleton, "hips").rotation;
      assert!(
        (turned - 0.5).abs() < 0.001,
        "placed again {again} times: {turned}"
      );
    }
  }
}
