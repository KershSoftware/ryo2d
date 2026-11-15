//! A skeleton in the world: the [`Skeleton`] component, what a game sets on
//! it (skins, attachments, bone overrides and constraint overrides, each
//! kept by name), and its part of the fixed step. The module notes give the
//! rules.

use std::sync::Arc;

use glam::{Affine2, Mat2, Vec2};

use super::apply::shortest;
use super::attachment::Attachment;
use super::constrain::{Constrained, constrain};
use super::constraint::{Constraint, ConstraintId, IkBend, MAX_CONSTRAINTS};
use super::data::{BoneId, BoneSet, DataVersion, SkeletonData, SkeletonIndex, SlotId};
use super::pose::{BonePose, ConstraintValues, Pose};
use super::scope::SkeletonStore;
use super::state::{FiredEvent, FiredTrackEvent, SkeletonAnimation};
use super::validate::{MAX_BONES, MAX_SLOTS};
use crate::core::asset::Handle;
use crate::core::ecs::kernel::{DeferredWorld, HookContext};
use crate::core::ecs::{Component, Interpolated, Renderable, Transform};

/// The most names a skeleton remembers having warned about. Past it, a
/// name the data lacks is ignored without a warning
const MAX_WARNED: usize = 64;

/// Below this, a transform's determinant counts as zero: nothing can be
/// converted through it
const FLAT: f32 = 1e-12;

/// The space a [`BoneOverride`]'s rotation and translation are in
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Space {
  /// The bone's own values, relative to its parent bone: what its setup
  /// pose and animations set
  #[default]
  Local,
  /// The world: a rotation is the way the bone points (the angle of its x
  /// axis, y down and positive clockwise), and a translation is where it
  /// is. Each is converted to the bone's parent's space every step
  World,
}

/// What a game sets on a bone after animation, to aim, look or lean:
/// [`Skeleton::override_bone`] keeps it until
/// [`Skeleton::clear_override`]. Each value given replaces the animated
/// one, by `mix`. The default sets nothing, at a mix of 1
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneOverride {
  /// The bone's rotation in radians, in `space`. It blends the short way
  /// round
  pub rotation: Option<f32>,
  /// The bone's position, in `space`
  pub translation: Option<Vec2>,
  /// The bone's own scale (always local, whatever `space` is)
  pub scale: Option<Vec2>,
  pub space: Space,
  /// How far each value moves from the animated one to the one given, 0 to
  /// 1 (1)
  pub mix: f32,
}

impl Default for BoneOverride {
  fn default() -> Self {
    Self {
      rotation: None,
      translation: None,
      scale: None,
      space: Space::Local,
      mix: 1.0,
    }
  }
}

impl BoneOverride {
  /// The override without its values that are not finite (a mix that is
  /// not finite is 0), its mix within 0 to 1, and whether any was taken out
  fn checked(mut self) -> (Self, bool) {
    let mut bad = false;
    if self.rotation.is_some_and(|rotation| !rotation.is_finite()) {
      self.rotation = None;
      bad = true;
    }
    if self.translation.is_some_and(|at| !at.is_finite()) {
      self.translation = None;
      bad = true;
    }
    if self.scale.is_some_and(|scale| !scale.is_finite()) {
      self.scale = None;
      bad = true;
    }
    if !self.mix.is_finite() {
      self.mix = 0.0;
      bad = true;
    }
    self.mix = self.mix.clamp(0.0, 1.0);
    (self, bad)
  }
}

/// What a game sets on a constraint after animation:
/// [`Skeleton::override_constraint`] keeps it until
/// [`Skeleton::clear_constraint_override`]. Its kind must be the
/// constraint's
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConstraintOverride {
  /// For an IK constraint: any of its mix, softness and switches
  Ik(IkOverride),
  /// For a transform constraint: any of its six mixes
  Transform(TransformMixOverride),
}

/// The values an override sets on an IK constraint. Each one given replaces
/// what the animations key (or the setup's), a switch included; each left
/// `None` is the animated one. The default sets nothing
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IkOverride {
  /// 0 (the animated pose) to 1 (the solved pose); taken within 0 to 1. At
  /// 0 the constraint is off: its bones are left to animation and the
  /// game's bone overrides
  pub mix: Option<f32>,
  /// Two bones: a distance from full reach within which the tip slows; not
  /// below 0
  pub softness: Option<f32>,
  /// Which way it bends: either way for two bones, a side or the pose's
  /// way for a chain
  pub bend: Option<IkBend>,
  /// One bone: scale it down when the target is nearer than its length
  pub compress: Option<bool>,
  /// One or two bones: scale the first along its length when the target is
  /// beyond reach
  pub stretch: Option<bool>,
  /// One or two bones: the most stretch takes the scale to, as a factor of
  /// the reach; at least 1, or it is dropped with a warning
  pub max_stretch: Option<f32>,
}

/// The mixes an override sets on a transform constraint. Each one given
/// replaces what the animations key (or the setup's); each left `None` is
/// the animated one. A mix may be below 0 or above 1. The default sets
/// nothing
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TransformMixOverride {
  pub rotate: Option<f32>,
  pub x: Option<f32>,
  pub y: Option<f32>,
  pub scale_x: Option<f32>,
  pub scale_y: Option<f32>,
  pub shear_y: Option<f32>,
}

impl ConstraintOverride {
  /// The override without its values that are not finite, an IK mix within
  /// 0 to 1, an IK softness not below 0 and an IK `max_stretch` not below 1,
  /// and whether any value was taken out
  fn checked(self) -> (Self, bool) {
    fn keep(value: &mut Option<f32>, bad: &mut bool) {
      if value.is_some_and(|value| !value.is_finite()) {
        *value = None;
        *bad = true;
      }
    }
    let mut bad = false;
    let checked = match self {
      Self::Ik(mut ik) => {
        keep(&mut ik.mix, &mut bad);
        keep(&mut ik.softness, &mut bad);
        keep(&mut ik.max_stretch, &mut bad);
        if ik.max_stretch.is_some_and(|max| max < 1.0) {
          ik.max_stretch = None;
          bad = true;
        }
        ik.mix = ik.mix.map(|mix| mix.clamp(0.0, 1.0));
        ik.softness = ik.softness.map(|softness| softness.max(0.0));
        Self::Ik(ik)
      }
      Self::Transform(mut mix) => {
        for value in [
          &mut mix.rotate,
          &mut mix.x,
          &mut mix.y,
          &mut mix.scale_x,
          &mut mix.scale_y,
          &mut mix.shear_y,
        ] {
          keep(value, &mut bad);
        }
        Self::Transform(mix)
      }
    };
    (checked, bad)
  }

  /// Whether it is of the kind of `values`, a constraint's values in a
  /// pose
  fn fits(&self, values: Option<&ConstraintValues>) -> bool {
    matches!(
      (self, values),
      (Self::Ik(_), Some(ConstraintValues::Ik(_)))
        | (Self::Transform(_), Some(ConstraintValues::Transform(_)))
    )
  }

  /// Writes the values it gives over `values`, when they are of its kind:
  /// those of a constraint of `bones` bones. A bend that many bones cannot
  /// have is passed over
  fn apply(&self, values: &mut ConstraintValues, bones: usize) {
    match (self, values) {
      (Self::Ik(o), ConstraintValues::Ik(values)) => {
        set(&mut values.mix, o.mix);
        set(&mut values.softness, o.softness);
        set(&mut values.bend, o.bend.filter(|bend| bend.fits(bones)));
        set(&mut values.compress, o.compress);
        set(&mut values.stretch, o.stretch);
        if bones <= 2 {
          set(&mut values.max_stretch, o.max_stretch.map(Some));
        }
      }
      (Self::Transform(o), ConstraintValues::Transform(values)) => {
        set(&mut values.rotate, o.rotate);
        set(&mut values.x, o.x);
        set(&mut values.y, o.y);
        set(&mut values.scale_x, o.scale_x);
        set(&mut values.scale_y, o.scale_y);
        set(&mut values.shear_y, o.shear_y);
      }
      _ => {}
    }
  }
}

/// Puts `given` over `value` when there is one
fn set<T>(value: &mut T, given: Option<T>) {
  if let Some(given) = given {
    *value = given;
  }
}

/// What kind of thing a name the data lacks was given as
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Named {
  Bone,
  Slot,
  Skin,
  Constraint,
  /// A constraint of the other kind than its override
  ConstraintKind,
}

/// A skin the game put on, and its place in the data's skins (none while
/// the data lacks it, or has not landed)
#[derive(Debug, Clone)]
struct WornSkin {
  name: Arc<str>,
  skin: Option<usize>,
}

/// An attachment the game set on a slot (`None` hides the slot)
#[derive(Debug, Clone)]
struct SetAttachment {
  slot: Arc<str>,
  id: Option<SlotId>,
  attachment: Option<Arc<str>>,
}

/// A bone override the game set, with the bone's pose as animation left it
/// in the last step
#[derive(Debug, Clone)]
pub(super) struct SetOverride {
  bone: Arc<str>,
  pub(super) id: Option<BoneId>,
  pub(super) value: BoneOverride,
  pub(super) animated: Option<BonePose>,
}

/// A constraint override the game set, with the constraint's place in the
/// data (none while the data lacks a constraint of that name and kind, or
/// has not landed)
#[derive(Debug, Clone)]
struct SetConstraintOverride {
  constraint: Arc<str>,
  id: Option<ConstraintId>,
  value: ConstraintOverride,
}

/// What a skeleton's step fired: the event keys its animations crossed, in
/// time order, and what happened to its track entries
#[derive(Debug, Default)]
pub(crate) struct Fired {
  pub(crate) events: Vec<FiredEvent>,
  pub(crate) track_events: Vec<FiredTrackEvent>,
}

/// Which bones of a new pose took their last world transform from the pose
/// before it
type Carried = BoneSet;

/// Which way a skeleton is mirrored about its entity's origin: the flips of
/// the [`Renderable`] that draws it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Flip {
  pub(crate) x: bool,
  pub(crate) y: bool,
}

impl Flip {
  /// The flips of the renderable that draws its entity's skeleton; none
  /// for a renderable that draws something else, or with no renderable
  pub(crate) fn of(renderable: Option<&Renderable>) -> Self {
    match renderable {
      Some(renderable) if renderable.is_skeleton() => Self {
        x: renderable.flip_x,
        y: renderable.flip_y,
      },
      _ => Self::default(),
    }
  }

  /// `space` mirrored about its origin along each flipped axis
  pub(crate) fn over(self, space: Affine2) -> Affine2 {
    if self == Self::default() {
      return space;
    }
    let scale = Vec2::new(
      if self.x { -1.0 } else { 1.0 },
      if self.y { -1.0 } else { 1.0 },
    );
    Affine2::from_scale(scale) * space
  }

  /// `root` with its own axes mirrored: what places a flipped skeleton's
  /// bones under its entity's transform
  pub(crate) fn under(self, root: Affine2) -> Affine2 {
    if self == Self::default() {
      return root;
    }
    let (x, y) = (root.matrix2.x_axis, root.matrix2.y_axis);
    Affine2::from_cols(
      if self.x { -x } else { x },
      if self.y { -y } else { y },
      root.translation,
    )
  }
}

/// Where a skeleton's bones are placed in the world: under its entity's
/// transform, mirrored about the entity's origin as its renderable flips
/// it
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stand {
  /// The entity's transform
  pub(crate) root: Affine2,
  pub(crate) flip: Flip,
}

impl Stand {
  /// What takes the skeleton's own space to the world
  pub(crate) fn affine(&self) -> Affine2 {
    self.flip.under(self.root)
  }
}

impl From<Affine2> for Stand {
  /// Under `root`, unflipped
  fn from(root: Affine2) -> Self {
    Self {
      root,
      flip: Flip::default(),
    }
  }
}

/// A skeleton on an entity: it shows [`SkeletonData`] in its setup pose,
/// or as the entity's [`SkeletonAnimation`] poses it, where the entity's
/// transform puts it. The engine steps it in each fixed step
/// (`FixedSet::Skeletons`); until its data has loaded it has no bones and
/// waits.
///
/// It has a pose from the frame it is added in, or its data lands in,
/// whether or not a fixed step runs that frame: its animation's first
/// pose when one is set. It advances only in fixed steps, so not while
/// the game clock is paused, and not while the scene that added it is
/// stopped under another, when it also sends nothing.
///
/// What a game sets on it (skins, attachments, bone and constraint
/// overrides) is its own, not the data's, and is kept by name. An
/// attachment or an override shows from the skeleton's next step, and
/// what it answers of its bones and slots (`bone_world`, `point`,
/// `bounding_box`, `attachment`) is as its last step left it. A skin
/// shows at once: the skins worn are looked up when the skeleton is drawn
/// and when it is asked, so the next frame draws a skin just put on, with
/// the game clock paused too, and `skins` gives what it wears.
///
/// It is drawn when its entity has a [`Renderable::skeleton`]: its regions
/// where its bones are and its meshes skinned to them, in the shapes its
/// animations' deform keys give them, sorted, lit, shadowed and lifted as
/// a sprite is, and mirrored about the entity's origin by that
/// renderable's `flip_x` and `flip_y`, its bones (as `bone_world` answers)
/// and sockets with it.
///
/// Adding a `Skeleton` makes its entity [`Interpolated`], so the entity,
/// its bones and its sockets are drawn between fixed steps, smooth on any
/// display. For a skeleton that animates at the step rate, remove
/// `Interpolated` from the entity once the skeleton is on it (a bundle
/// cannot leave it out); inserting a `Skeleton` again adds it again.
///
/// To give an entity another skeleton, insert the new `Skeleton` over the
/// old one. Assigning a whole new value through a mutable reference
/// (`*skeleton = Skeleton::new(..)`) does not count as adding one: no
/// scene is recorded for it, and it has no pose until its next step
#[derive(Component, Debug, Clone)]
#[require(Transform, Interpolated)]
#[component(on_insert = skeleton_inserted)]
pub struct Skeleton {
  data: Handle<SkeletonData>,
  /// The scene whose code added it, which it is held with; `None` for the
  /// engine's own code, and once that scene has exited
  scope: Option<u64>,
  /// The skins worn, the later over the earlier, all over the default skin
  skins: Vec<WornSkin>,
  attachments: Vec<SetAttachment>,
  /// In bone order (parents first), those of bones the data lacks last
  overrides: Vec<SetOverride>,
  constraint_overrides: Vec<SetConstraintOverride>,
  /// What applying the data's constraints keeps from step to step; nothing
  /// for data without constraints
  pub(super) constrained: Constrained,
  /// The data's id and version that the pose and the names' places were
  /// made from
  seen: Option<DataVersion>,
  index: Option<Arc<SkeletonIndex>>,
  /// The pose as the last step left it; nothing in it until the data lands
  pub(crate) pose: Pose,
  /// The bones' transforms in the skeleton's own space a step before
  /// `pose.skeleton_space`'s, for drawing between steps. A bone the last
  /// step first placed has its current one, and so has every bone after a
  /// pose made outside a step, or a step that follows one it missed
  pub(crate) previous_space: Vec<Affine2>,
  /// The entity's transform that `pose.world` was last computed from, and
  /// the way it was mirrored
  root: Affine2,
  flip: Flip,
  /// The number of the fixed step that last stepped it, or after which it
  /// was last posed outside a step (0: none)
  stepped: u64,
  /// The numbers of its last two placings: each new pose, for its sockets
  /// to be placed on, takes a number (0: none yet)
  placing: u64,
  placed_before: u64,
  warned: Vec<(Named, Arc<str>)>,
  warned_value: bool,
  warned_constraint_value: bool,
  warned_full: bool,
}

impl Skeleton {
  /// A skeleton of `data`, in its default skin
  pub fn new(data: Handle<SkeletonData>) -> Self {
    Self {
      data,
      scope: None,
      skins: Vec::new(),
      attachments: Vec::new(),
      overrides: Vec::new(),
      constraint_overrides: Vec::new(),
      constrained: Constrained::default(),
      seen: None,
      index: None,
      pose: Pose::default(),
      previous_space: Vec::new(),
      root: Affine2::IDENTITY,
      flip: Flip::default(),
      stepped: 0,
      placing: 0,
      placed_before: 0,
      warned: Vec::new(),
      warned_value: false,
      warned_constraint_value: false,
      warned_full: false,
    }
  }

  /// The data it shows
  pub fn data(&self) -> &Handle<SkeletonData> {
    &self.data
  }

  /// The world transform of the bone called `name`, as the skeleton's last
  /// step left it: its origin, and its x and y axes with the scale and
  /// shear its parents and the entity give it. `None` for a name the
  /// skeleton lacks, and until the data has loaded and a step has run.
  ///
  /// Read from a scene's `update`, it is one step behind that step's
  /// motion: the skeleton's step runs after the scene's `update` in each
  /// fixed step (and its `SkeletonEvent`s likewise arrive in the next
  /// step's `update`). For the bone in the same step, use a
  /// [`BoneSocket`](super::BoneSocket), or a system ordered after
  /// `FixedSet::Skeletons` and added with `EngineBuilder::add_systems`: a
  /// scene's own `add_systems` runs its systems before the skeletons'
  /// step, and refuses a call that names an engine set
  pub fn bone_world(&self, name: &str) -> Option<Affine2> {
    let bone = self.index.as_ref()?.bone_names.get(name)?;
    self.pose.world.get(bone.index()).copied()
  }

  /// Wears the skin called `name` alone, over the default skin. A name the
  /// data lacks warns once per skeleton per name and changes nothing
  pub fn set_skin(&mut self, name: &str) {
    self.set_skins(&[name]);
  }

  /// Wears several skins as one: an attachment is looked up in the last of
  /// them that has it, then in the default skin. An empty list leaves the
  /// default skin alone. A name the data lacks warns once per skeleton per
  /// name and is left out; if the data has none of the names, nothing
  /// changes. The skins show at once, in the next frame drawn and in what
  /// the skeleton answers, without waiting for a fixed step
  pub fn set_skins(&mut self, names: &[&str]) {
    let Some(index) = self.index.clone() else {
      // Kept by name until the data lands
      let same = self.skins.len() == names.len()
        && self
          .skins
          .iter()
          .zip(names)
          .all(|(worn, name)| &*worn.name == *name);
      if !same {
        self.skins.clear();
        self.skins.extend(names.iter().map(|name| WornSkin {
          name: Arc::from(*name),
          skin: None,
        }));
      }
      return;
    };
    let mut known = 0;
    for name in names {
      if index.skin_names.contains_key(*name) {
        known += 1;
      } else {
        self.warn_missing(Named::Skin, name);
      }
    }
    if known == 0 && !names.is_empty() {
      return;
    }
    self.skins.clear();
    for name in names {
      if let Some((name, &skin)) = index.skin_names.get_key_value(*name) {
        self.skins.push(WornSkin {
          name: name.clone(),
          skin: Some(skin),
        });
      }
    }
  }

  /// The names of the skins worn, in the order they were put on (the later
  /// over the earlier); the default skin, under them all, is not among
  /// them. It is as of the skeleton's last step, with the calls made
  /// since: [`set_skin`](Self::set_skin) and
  /// [`set_skins`](Self::set_skins) read back at once. Until the data has
  /// loaded it gives the names kept for it, which cannot be checked yet;
  /// from then on, only the skins the data has. Nothing is allocated
  pub fn skins(&self) -> impl Iterator<Item = &str> {
    let landed = self.index.is_some();
    self
      .skins
      .iter()
      .filter(move |worn| !landed || worn.skin.is_some())
      .map(|worn| &*worn.name)
  }

  /// Shows the attachment called `attachment` in the slot called `slot`
  /// (`None` hides the slot), whatever animations key there, until
  /// [`clear_attachment`](Self::clear_attachment) hands the slot back. The
  /// name is looked up in the skins worn, like an animation's: one no skin
  /// has in the slot shows nothing. A slot the data lacks warns once per
  /// skeleton per name and does nothing
  pub fn set_attachment(&mut self, slot: &str, attachment: Option<&str>) {
    if let Some(at) = self.attachments.iter().position(|set| &*set.slot == slot) {
      if self.attachments[at].attachment.as_deref() != attachment {
        let id = self.attachments[at].id;
        let attachment = attachment.map(|name| self.attachment_name(id, name));
        self.attachments[at].attachment = attachment;
      }
      return;
    }
    if !self.room(self.attachments.len(), MAX_SLOTS) {
      return;
    }
    let (slot, id) = match &self.index {
      None => (Arc::from(slot), None),
      Some(index) => match index.slot_names.get_key_value(slot) {
        Some((name, &id)) => (name.clone(), Some(id)),
        None => {
          self.warn_missing(Named::Slot, slot);
          return;
        }
      },
    };
    let attachment = attachment.map(|name| self.attachment_name(id, name));
    self.attachments.push(SetAttachment {
      slot,
      id,
      attachment,
    });
  }

  /// Hands the slot called `slot` back to animation: from the next step it
  /// shows what the animations key there, else its setup attachment
  pub fn clear_attachment(&mut self, slot: &str) {
    self.attachments.retain(|set| &*set.slot != slot);
  }

  /// The name of the attachment the slot called `slot` shows, as of the
  /// skeleton's last step: what the game set there, else what its
  /// animations keyed, else its setup attachment. So an attachment set
  /// with [`set_attachment`](Self::set_attachment) reads back here after
  /// the skeleton's next step. `None` when the slot shows nothing (it is
  /// hidden, or no skin worn has an attachment of that name for it), for a
  /// slot the skeleton lacks, and until the data has loaded. The name is
  /// the last step's, but whether a skin worn has it is looked up as this
  /// is asked, so [`set_skins`](Self::set_skins) changes the answer at
  /// once
  pub fn attachment(&self, slot: &str) -> Option<&str> {
    let slot = *self.index.as_ref()?.slot_names.get(slot)?;
    let name = self.pose.slot_attachments.get(slot.index())?.as_deref()?;
    self.skin_attachment(slot, name).map(|_| name)
  }

  /// Sets the bone called `bone` after animation, every step until
  /// [`clear_override`](Self::clear_override): each value given replaces
  /// the animated one by the override's `mix`, and the bone's children
  /// follow. Setting a bone again replaces its override. A value that is
  /// not finite is ignored (a mix that is not finite counts as 0), with
  /// one warning per skeleton. A bone the data lacks warns once per
  /// skeleton per name and does nothing
  pub fn override_bone(&mut self, bone: &str, o: BoneOverride) {
    let (value, bad) = o.checked();
    if bad && !self.warned_value {
      self.warned_value = true;
      log::warn!(
        "skeleton bone override for {bone:?} has a value that is not finite; the value is ignored"
      );
    }
    if let Some(set) = self.overrides.iter_mut().find(|set| &*set.bone == bone) {
      set.value = value;
      return;
    }
    if !self.room(self.overrides.len(), MAX_BONES) {
      return;
    }
    let (bone, id) = match &self.index {
      None => (Arc::from(bone), None),
      Some(index) => match index.bone_names.get_key_value(bone) {
        Some((name, &id)) => (name.clone(), Some(id)),
        None => {
          self.warn_missing(Named::Bone, bone);
          return;
        }
      },
    };
    self.overrides.push(SetOverride {
      bone,
      id,
      value,
      animated: None,
    });
    self.sort_overrides();
  }

  /// Takes the override off the bone called `bone`: from the next step it
  /// is the animation's again
  pub fn clear_override(&mut self, bone: &str) {
    self.overrides.retain(|set| &*set.bone != bone);
  }

  /// Sets the constraint called `name` after animation, every step until
  /// [`clear_constraint_override`](Self::clear_constraint_override): each
  /// value given replaces what the animations key, or the setup's where
  /// none does, a switch included; each left `None` is the animated one.
  /// It shows from the skeleton's next step. Setting a constraint again
  /// replaces its override. A mix of 0 turns the constraint off for as
  /// long as it is set, leaving its bones to animation and the game's bone
  /// overrides (an IK constraint is off at a mix of 0, a transform
  /// constraint with every mix at 0): how a game lets a foot go in the air.
  ///
  /// An IK mix is taken within 0 to 1 and a softness below 0 as 0. A value
  /// that is not finite is ignored, with one warning per skeleton. A switch
  /// the constraint cannot have (`stretch` on a chain, `compress` on two
  /// bones or a chain, a `softness` on other than two bones, a `bend` of
  /// `Pose` on two bones or any but `Positive` on one) is ignored in
  /// silence.
  ///
  /// A name the data lacks, or a constraint of the other kind, warns once
  /// per skeleton per name and does nothing to the constraint, nor to an
  /// override it already has. Until the data lands the override is kept by
  /// name, and through a reload too: one kept for a name the data lacks, or
  /// has as the other kind, is updated by a later call all the same, and
  /// applies once data that has the constraint lands
  pub fn override_constraint(&mut self, name: &str, o: ConstraintOverride) {
    let (value, bad) = o.checked();
    if bad && !self.warned_constraint_value {
      self.warned_constraint_value = true;
      log::warn!(
        "skeleton constraint override for {name:?} has a value that is not finite or out of range; the value is ignored"
      );
    }
    let at = self
      .constraint_overrides
      .iter()
      .position(|set| &*set.constraint == name);
    // Kept by name until the data lands; after, found by name and kind
    let (constraint, id, missing) = match &self.index {
      None => (None, None, None),
      Some(index) => match index.constraint_names.get_key_value(name) {
        Some((known, &id)) if self.fits(id, &value) => (Some(known.clone()), Some(id), None),
        Some(_) => (None, None, Some(Named::ConstraintKind)),
        None => (None, None, Some(Named::Constraint)),
      },
    };
    if let Some(missing) = missing {
      self.warn_missing(missing, name);
    }
    if let Some(set) = at.and_then(|at| self.constraint_overrides.get_mut(at)) {
      // One the data has, of the right kind, is not replaced by a mistake;
      // one kept for a later version (a name the data lacks, or has as
      // the other kind) takes the new value for when that version lands
      if missing.is_none() || set.id.is_none() {
        set.value = value;
        set.id = id;
      }
      return;
    }
    if missing.is_some() {
      return;
    }
    if !self.room(self.constraint_overrides.len(), MAX_CONSTRAINTS) {
      return;
    }
    self.constraint_overrides.push(SetConstraintOverride {
      constraint: constraint.unwrap_or_else(|| Arc::from(name)),
      id,
      value,
    });
  }

  /// Takes the override off the constraint called `name`: from the next
  /// step it is the animation's again
  pub fn clear_constraint_override(&mut self, name: &str) {
    self
      .constraint_overrides
      .retain(|set| &*set.constraint != name);
  }

  /// Whether `value` is of the kind of the constraint at `id` in the data
  /// the pose was made from
  fn fits(&self, id: ConstraintId, value: &ConstraintOverride) -> bool {
    value.fits(self.pose.constraints.get(id.index()))
  }

  /// The point attachment called `name` that one of the skeleton's slots
  /// shows (the first such slot in setup order), in the world: where it is
  /// and the way it points, in radians. `None` when no slot shows a point
  /// of that name: a point an animation or the game has hidden is not
  /// found. Read from a scene's `update`, it is as the skeleton's last step
  /// left it, one step behind that step's motion (see
  /// [`bone_world`](Self::bone_world)); for the same step, use a
  /// [`BoneSocket`](super::BoneSocket), or a system ordered after
  /// `FixedSet::Skeletons` and added with `EngineBuilder::add_systems` (a
  /// scene's own `add_systems` cannot order one there)
  pub fn point(&self, name: &str) -> Option<(Vec2, f32)> {
    self.shown(name).find_map(|(world, attachment)| {
      let Attachment::Point(point) = attachment else {
        return None;
      };
      let at = world.transform_point2(Vec2::new(point.x, point.y));
      let direction = world.matrix2 * Vec2::from_angle(point.rotation);
      Some((at, direction.to_angle()))
    })
  }

  /// Fills `out` with the corners, in the world, of the bounding box
  /// called `name` that one of the skeleton's slots shows (the first such
  /// slot in setup order), replacing what `out` held. False, leaving `out`
  /// empty, when no slot shows a bounding box of that name: an animation
  /// can so turn a hit box on and off by keying its slot's attachment.
  /// Read from a scene's `update`, the corners are as the skeleton's last
  /// step left them, one step behind that step's motion (see
  /// [`bone_world`](Self::bone_world)); for the same step, read them in a
  /// system ordered after `FixedSet::Skeletons` and added with
  /// `EngineBuilder::add_systems` (a scene's own `add_systems` cannot
  /// order one there)
  pub fn bounding_box(&self, name: &str, out: &mut Vec<Vec2>) -> bool {
    out.clear();
    let found = self
      .shown(name)
      .find_map(|(world, attachment)| match attachment {
        Attachment::BoundingBox(bounds) => Some((world, bounds)),
        _ => None,
      });
    let Some((world, bounds)) = found else {
      return false;
    };
    out.extend(
      bounds
        .vertices
        .iter()
        .map(|&corner| world.transform_point2(corner)),
    );
    true
  }

  /// The attachment called `name` in `slot`: the last skin worn that has
  /// one there, else the default skin's
  pub(crate) fn skin_attachment(&self, slot: SlotId, name: &str) -> Option<&Attachment> {
    worn(self.index.as_deref()?, &self.skins, slot, name)
  }

  /// The place of the bone called `name` in the data the pose was made
  /// from
  pub(crate) fn bone_id(&self, name: &str) -> Option<BoneId> {
    self.index.as_ref()?.bone_names.get(name).copied()
  }

  /// A bone's transform relative to the skeleton's entity, as the last
  /// step left it: its transform in the skeleton's own space, mirrored as
  /// the skeleton is. What a socket on the bone is placed by
  pub(crate) fn bone_space(&self, bone: BoneId) -> Option<Affine2> {
    let space = self.pose.skeleton_space.get(bone.index())?;
    Some(self.flip.over(*space))
  }

  /// It has a new pose (a step, or a pose outside one) for its sockets to
  /// be placed on: the placing takes `number`, or with none the next after
  /// its last
  pub(crate) fn new_placing(&mut self, number: Option<u64>) {
    self.placed_before = self.placing;
    self.placing = number.unwrap_or(self.placing + 1);
  }

  /// The number of its latest placing, and of the one before
  pub(crate) fn placings(&self) -> [u64; 2] {
    [self.placing, self.placed_before]
  }

  /// The scene it belongs to
  pub(crate) fn scope(&self) -> Option<u64> {
    self.scope
  }

  /// Gives it to another scene, or to none
  pub(crate) fn set_scope(&mut self, scope: Option<u64>) {
    self.scope = scope;
  }

  /// The data's id and version its pose was made from, once a step has
  /// seen the data
  pub(crate) fn seen(&self) -> Option<DataVersion> {
    self.seen
  }

  /// The entity's transform its bones' world transforms were last computed
  /// from
  pub(crate) fn root(&self) -> Affine2 {
    self.root
  }

  /// The way it was mirrored when its bones were last placed
  pub(crate) fn flip(&self) -> Flip {
    self.flip
  }

  /// The number of the fixed step that last stepped it, or after which it
  /// was last posed outside a step
  pub(crate) fn stepped(&self) -> u64 {
    self.stepped
  }

  /// Its step was fixed step `number` (the store's count). One that missed
  /// the step before (disabled, held, or waiting for its data) has no pose
  /// of that step to be drawn from: this step is a first placing, each
  /// bone's previous transform its current one
  pub(crate) fn took_step(&mut self, number: u64) {
    if self.stepped.wrapping_add(1) != number {
      self.settle();
    }
    self.stepped = number;
  }

  /// It was posed outside a step, after fixed step `latest`: its next step
  /// follows on from that pose
  pub(crate) fn posed_after(&mut self, latest: u64) {
    self.stepped = latest;
  }

  /// Nothing is drawn between its last two poses: each bone's previous
  /// transform becomes its current one, and each mesh's previous deform
  /// offsets its current ones. For a step that follows one it missed, a
  /// pose made outside a step, and a pose it was flipped in: its sockets
  /// land across it then, and a bone aimed in the world is solved afresh
  /// on the other side, so its bones land too
  pub(crate) fn settle(&mut self) {
    self.previous_space.clone_from(&self.pose.skeleton_space);
    self.pose.settle_deforms();
  }

  /// One fixed step. A step that sees other data than the last (the data
  /// landing, or a reload) first makes the pose anew and finds every kept
  /// name's place in it. Then: the animation state advances by `dt` and
  /// poses the skeleton (without one it holds its setup pose), the game's
  /// attachments are put over that, the animations' deform keys move the
  /// meshes the slots then show, the game's constraint overrides and
  /// bone overrides are applied, the data's constraints move their bones,
  /// in order, and the bones' world transforms are computed where it
  /// stands (`stand`: under the entity's transform, mirrored as its
  /// renderable flips it). The event keys crossed and the track events are
  /// pushed onto `fired`
  pub(crate) fn step(
    &mut self,
    data: &SkeletonData,
    seen: DataVersion,
    mut animation: Option<&mut SkeletonAnimation>,
    dt: f32,
    stand: Stand,
    fired: &mut Fired,
  ) {
    let mut carried = None;
    if self.seen == Some(seen) {
      // The transforms and the deform offsets the last step computed
      // become the previous ones, and their buffers are computed over
      std::mem::swap(&mut self.previous_space, &mut self.pose.skeleton_space);
      self.pose.step_deforms();
    } else {
      carried = Some(self.rebuild(data));
      self.seen = Some(seen);
    }
    match animation.as_deref_mut() {
      Some(animation) => animation.advance_and_apply(
        data,
        &mut self.pose,
        dt,
        &mut fired.events,
        &mut fired.track_events,
      ),
      None => self.pose.reset(data),
    }
    self.finish(data, animation, stand, carried);
  }

  /// A pose outside a step, for a skeleton that has none for `data` as it
  /// now is (just added, or its data just landed or reloaded): the pose is
  /// made anew and the animation state poses it where its tracks stand,
  /// an entry that has yet to start at its first moment (its `start`).
  /// Nothing advances and nothing is fired: an entry's `Start` and the keys
  /// at its time 0 go out with its first step, which advances it from its
  /// start as if this had not run. With no state it is the setup pose. No
  /// step made it, so it is drawn as it is, not between it and the pose
  /// before
  pub(crate) fn appear(
    &mut self,
    data: &SkeletonData,
    seen: DataVersion,
    mut animation: Option<&mut SkeletonAnimation>,
    stand: Stand,
  ) {
    self.rebuild(data);
    self.seen = Some(seen);
    match animation.as_deref_mut() {
      Some(animation) => animation.apply_unadvanced(data, &mut self.pose),
      None => self.pose.reset(data),
    }
    self.finish(data, animation, stand, None);
    self.settle();
  }

  /// The end of a step and of a pose outside one: what the game set is put
  /// over the animation's result, the deform keys of `animation`, which
  /// posed it, move the meshes its slots then show, the game's constraint
  /// overrides are put over the constraints' animated values, and the
  /// bones are placed where it stands, the data's constraints applied.
  /// After new data (`carried`), a bone with no transform from the pose
  /// before takes its current one as its previous one
  fn finish(
    &mut self,
    data: &SkeletonData,
    animation: Option<&mut SkeletonAnimation>,
    stand: Stand,
    carried: Option<Carried>,
  ) {
    // The animation state starts each pose from the setup pose, so what
    // the game set is put over its result every time
    for set in &self.attachments {
      if let Some(shown) = set
        .id
        .and_then(|slot| self.pose.slot_attachments.get_mut(slot.index()))
      {
        shown.clone_from(&set.attachment);
      }
    }
    // The slots now show what this pose leaves them showing, in the skins
    // worn: a deform key moves the mesh it finds there, or nothing
    if let Some(animation) = animation {
      let (index, skins) = (self.index.as_deref(), &self.skins);
      animation.apply_deforms(data, &mut self.pose, |slot, name| {
        worn(index?, skins, slot, name)
      });
    }
    self.pose.deforms_follow_slots();
    // What animation left, for a placing again to start from
    self.constrained.keep(&self.pose);
    for set in &mut self.overrides {
      set.animated = set
        .id
        .and_then(|bone| self.pose.bones.get(bone.index()).copied());
    }
    self.apply_constraint_overrides(data);
    self.place(data, stand);
    if let Some(carried) = carried {
      // A bone with no transform from the pose before draws where it is
      let placed = self
        .previous_space
        .iter_mut()
        .zip(&self.pose.skeleton_space);
      for (bone, (previous, space)) in placed.enumerate() {
        if !carried.has(bone) {
          *previous = *space;
        }
      }
    }
  }

  /// Writes the game's constraint overrides over the constraints' values
  /// as animation left them, for the constraint pass in `place`
  fn apply_constraint_overrides(&mut self, data: &SkeletonData) {
    for set in &self.constraint_overrides {
      let Some(constraint) = set.id else {
        continue;
      };
      let bones = match data.constraints().get(constraint.index()) {
        Some(Constraint::Ik(ik)) => ik.bones.len(),
        _ => 0,
      };
      if let Some(values) = self.pose.constraints.get_mut(constraint.index()) {
        set.value.apply(values, bones);
      }
    }
  }

  /// Computes the bones' world transforms again where it now stands, with
  /// the pose it has: the entity was moved after its skeleton's step (it
  /// sits on another skeleton's socket), or was flipped while no step
  /// runs. The bone overrides, the constraint overrides and the
  /// constraints are applied afresh, from the bones' own values and the
  /// constraints' values as animation left them: a world-space override
  /// depends on where it stands, a constraint on where that puts its
  /// target, and an override set or cleared since the step shows at once.
  /// So it gives the pose a step gives there, however often it runs
  pub(crate) fn place_again(&mut self, data: &SkeletonData, stand: Stand) {
    self.constrained.restore(&mut self.pose);
    self.apply_constraint_overrides(data);
    for set in &mut self.overrides {
      // One set since the last step has no animated values yet: it takes
      // them from the pose as it stands, so it is applied once, and afresh
      // below a constraint, as a step would apply it
      if set.animated.is_none() {
        set.animated = set
          .id
          .and_then(|bone| self.pose.bones.get(bone.index()).copied());
      }
      if let (Some(bone), Some(animated)) = (set.id, set.animated)
        && let Some(pose) = self.pose.bones.get_mut(bone.index())
      {
        *pose = animated;
      }
    }
    self.place(data, stand);
  }

  /// Applies the bone overrides and computes the bones' transforms, in one
  /// pass from the roots out: a world-space override is converted through
  /// its bone's parent, which is placed by then, overrides and all. Data
  /// with constraints then has them applied, in order, to the bones as
  /// that pass placed them in the skeleton's space, before the bones are
  /// put in the world
  fn place(&mut self, data: &SkeletonData, stand: Stand) {
    self.root = stand.root;
    self.flip = stand.flip;
    let root = stand.affine();
    if data.constraints().is_empty() {
      // Those of bones the data has come first: with none, the plain pass
      if self.overrides.first().is_none_or(|set| set.id.is_none()) {
        self.pose.compute_world(data, root);
        return;
      }
      self.place_overridden(data, root);
      self.pose.apply_root(root);
      return;
    }
    self.place_overridden(data, root);
    constrain(
      &mut self.pose,
      data,
      root,
      &self.overrides,
      &mut self.constrained,
    );
    self.pose.apply_root(root);
  }

  /// Places every bone in the skeleton's space, parents first, each bone
  /// the game overrides set just before it is placed
  fn place_overridden(&mut self, data: &SkeletonData, root: Affine2) {
    let mut next = 0;
    for bone in 0..data.bones().len() {
      if let Some(set) = self.overrides.get(next)
        && set.id.is_some_and(|id| id.index() == bone)
      {
        apply_override(&mut self.pose, data, bone, &set.value, root);
        next += 1;
      }
      self.pose.place_bone(data, bone);
    }
  }

  /// Makes the pose anew for `data`, with what applying its constraints
  /// keeps, and finds each kept name's place in it. A bone the pose before
  /// had, by name, keeps its last transform in the skeleton's space as its
  /// previous one; the rest are marked not carried
  fn rebuild(&mut self, data: &SkeletonData) -> Carried {
    let index = data.index().clone();
    let pose = Pose::setup(data);
    self.constrained = Constrained::new(data);
    let mut previous = vec![Affine2::IDENTITY; data.bones().len()];
    let mut carried = Carried::default();
    if let Some(old) = &self.index {
      for (name, bone) in &index.bone_names {
        let last = old
          .bone_names
          .get(name)
          .and_then(|was| self.pose.skeleton_space.get(was.index()));
        if let (Some(last), Some(previous)) = (last, previous.get_mut(bone.index())) {
          *previous = *last;
          carried.set(bone.index());
        }
      }
    }
    self.pose = pose;
    self.previous_space = previous;
    for at in 0..self.skins.len() {
      let name = self.skins[at].name.clone();
      self.skins[at].skin = index.skin_names.get(&*name).copied();
      if self.skins[at].skin.is_none() {
        self.warn_missing(Named::Skin, &name);
      }
    }
    for at in 0..self.attachments.len() {
      let name = self.attachments[at].slot.clone();
      self.attachments[at].id = index.slot_names.get(&*name).copied();
      if self.attachments[at].id.is_none() {
        self.warn_missing(Named::Slot, &name);
      }
    }
    for at in 0..self.overrides.len() {
      let name = self.overrides[at].bone.clone();
      self.overrides[at].id = index.bone_names.get(&*name).copied();
      self.overrides[at].animated = None;
      if self.overrides[at].id.is_none() {
        self.warn_missing(Named::Bone, &name);
      }
    }
    self.sort_overrides();
    for at in 0..self.constraint_overrides.len() {
      let set = &self.constraint_overrides[at];
      let (name, value) = (set.constraint.clone(), set.value);
      let found = index.constraint_names.get(&*name).copied();
      let id = found.filter(|&id| self.fits(id, &value));
      self.constraint_overrides[at].id = id;
      match (found, id) {
        (None, _) => self.warn_missing(Named::Constraint, &name),
        (Some(_), None) => self.warn_missing(Named::ConstraintKind, &name),
        _ => {}
      }
    }
    self.index = Some(index);
    carried
  }

  /// Keeps the overrides in bone order, those of bones the data lacks last
  fn sort_overrides(&mut self) {
    self
      .overrides
      .sort_unstable_by_key(|set| set.id.map_or(u16::MAX, |bone| u16::from(bone.0)));
  }

  /// An attachment's name to keep: the data's own copy when a skin has it
  /// in the slot, so that setting one allocates nothing
  fn attachment_name(&self, slot: Option<SlotId>, name: &str) -> Arc<str> {
    self
      .index
      .as_ref()
      .zip(slot)
      .and_then(|(index, slot)| {
        index
          .skins
          .iter()
          .find_map(|skin| skin.slots.get(slot.index())?.get_key_value(name))
      })
      .map_or_else(|| Arc::from(name), |(name, _)| name.clone())
  }

  /// The slots showing an attachment called `name`, in setup order: each
  /// one's bone's world transform and the attachment
  fn shown<'a>(&'a self, name: &'a str) -> impl Iterator<Item = (Affine2, &'a Attachment)> + 'a {
    let slots = self.pose.slot_attachments.iter().enumerate();
    slots.filter_map(move |(slot, shown)| {
      if shown.as_deref() != Some(name) {
        return None;
      }
      let bone = self.index.as_ref()?.slot_bones.get(slot)?;
      let world = self.pose.world.get(bone.index())?;
      let slot = u8::try_from(slot).ok().map(SlotId)?;
      Some((*world, self.skin_attachment(slot, name)?))
    })
  }

  /// Whether one more of `count` attachments or overrides can be kept: a
  /// skeleton has at most `most` slots, bones or constraints to set them
  /// on, so past that the names are ones no data can have (set before the
  /// data landed, when they cannot be checked). One more is ignored, with
  /// one warning
  fn room(&mut self, count: usize, most: usize) -> bool {
    if count < most {
      return true;
    }
    if !self.warned_full {
      self.warned_full = true;
      log::warn!(
        "skeleton has {most} attachments, bone overrides or constraint overrides set on names its data has yet to confirm; more are ignored"
      );
    }
    false
  }

  /// Warns that the data lacks `name`, once per skeleton per name
  fn warn_missing(&mut self, kind: Named, name: &str) {
    let warned = |(was, known): &(Named, Arc<str>)| *was == kind && &**known == name;
    if self.warned.iter().any(warned) || self.warned.len() >= MAX_WARNED {
      return;
    }
    self.warned.push((kind, Arc::from(name)));
    match kind {
      Named::Bone => {
        log::warn!("skeleton bone {name:?} is not in the skeleton's data; its override is ignored");
      }
      Named::Slot => {
        log::warn!(
          "skeleton slot {name:?} is not in the skeleton's data; its attachment is ignored"
        );
      }
      Named::Skin => log::warn!("skeleton skin {name:?} is not in the skeleton's data; ignored"),
      Named::Constraint => {
        log::warn!(
          "skeleton constraint {name:?} is not in the skeleton's data; its override is ignored"
        );
      }
      Named::ConstraintKind => {
        log::warn!(
          "skeleton constraint {name:?} is not of its override's kind (IK or transform); the override is ignored"
        );
      }
    }
  }
}

/// The attachment called `name` in `slot` among `index`'s skins: the last of
/// the `skins` worn that has one there, else the default skin's
fn worn<'a>(
  index: &'a SkeletonIndex,
  skins: &[WornSkin],
  slot: SlotId,
  name: &str,
) -> Option<&'a Attachment> {
  skins
    .iter()
    .rev()
    .filter_map(|worn| worn.skin)
    .chain(std::iter::once(0))
    .find_map(|skin| index.skins.get(skin)?.attachment(slot, name))
}

/// A skeleton was added, or put over another: it belongs to the scene
/// whose code runs now, and is held with it; and the store counts it, so
/// the frame's pass looks for it. Nothing without a store
fn skeleton_inserted(mut world: DeferredWorld, context: HookContext) {
  let Some(scope) = world
    .get_resource_mut::<SkeletonStore>()
    .map(|mut store| store.add())
  else {
    return;
  };
  if let Some(mut skeleton) = world.get_mut::<Skeleton>(context.entity) {
    skeleton.scope = scope;
  }
}

/// Puts `value` over bone `bone`'s animated pose, by its mix. The bone's
/// parent must have been placed: a world-space translation is converted
/// through the parent's transform under `root`, and a world-space rotation
/// through the frame that carries the bone's own direction to the world.
/// One that cannot be converted (the frame is flat: a zero scale above the
/// bone) is left out
pub(super) fn apply_override(
  pose: &mut Pose,
  data: &SkeletonData,
  bone: usize,
  value: &BoneOverride,
  root: Affine2,
) {
  let mix = value.mix;
  if mix <= 0.0 {
    return;
  }
  let world = value.space == Space::World;
  let translation = value.translation.and_then(|at| {
    if !world {
      return Some(at);
    }
    let parent = root * pose.parent_space(data, bone);
    invertible(parent.matrix2).then(|| parent.inverse().transform_point2(at))
  });
  let frame = root.matrix2 * pose.direction_space(data, bone);
  let Some(local) = pose.bones.get_mut(bone) else {
    return;
  };
  if let Some(at) = translation {
    local.x += (at.x - local.x) * mix;
    local.y += (at.y - local.y) * mix;
  }
  if let Some(scale) = value.scale {
    local.scale_x += (scale.x - local.scale_x) * mix;
    local.scale_y += (scale.y - local.scale_y) * mix;
  }
  let rotation = value.rotation.and_then(|angle| {
    if !world {
      return Some(angle);
    }
    if !invertible(frame) {
      return None;
    }
    // The bone's x axis points along its own direction, turned by its
    // rotation and x shear and carried by the frame, or the other way under
    // a negative x scale
    let along = if local.scale_x < 0.0 { -1.0 } else { 1.0 };
    let own = frame.inverse() * (Vec2::from_angle(angle) * along);
    Some(own.to_angle() - local.shear_x)
  });
  if let Some(rotation) = rotation {
    local.rotation += shortest(rotation - local.rotation) * mix;
  }
}

/// Whether points and directions can be converted back through `linear`
pub(super) fn invertible(linear: Mat2) -> bool {
  let determinant = linear.determinant();
  determinant.is_finite() && determinant.abs() > FLAT
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;

  use super::*;
  use crate::core::asset::Assets;
  use crate::core::ecs::World;
  use crate::core::skeleton::{
    Animation, BoneSetup, Inherit, PointAttachment, SlotSetup, Timeline,
  };
  use crate::testing::{asset_world, capture_log};

  /// A skeleton of `data`, stepped once under `root` with `set` applied
  fn posed(data: SkeletonData, root: Affine2, set: impl FnOnce(&mut Skeleton)) -> Skeleton {
    let mut world = asset_world();
    let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
    let assets = world.resource::<Assets<SkeletonData>>();
    let data = assets.get(&handle).expect("the data");
    let version = assets.version(&handle).expect("a version");
    let mut skeleton = Skeleton::new(handle.clone());
    set(&mut skeleton);
    let seen = (handle.id(), version);
    skeleton.step(
      data,
      seen,
      None,
      1.0 / 60.0,
      root.into(),
      &mut Fired::default(),
    );
    skeleton
  }

  /// A parent at (10, 0) turned a quarter turn and scaled (2, 1), and a
  /// child 5 along it, turned 0.3, that inherits by `inherit`
  fn pair(inherit: Inherit) -> SkeletonData {
    pair_with(BoneSetup {
      x: 5.0,
      rotation: 0.3,
      inherit,
      ..BoneSetup::default()
    })
  }

  fn pair_with(child: BoneSetup) -> SkeletonData {
    let parent = BoneSetup {
      x: 10.0,
      rotation: FRAC_PI_2,
      scale_x: 2.0,
      ..BoneSetup::default()
    };
    SkeletonData::builder()
      .bone("parent", None, parent)
      .bone("child", Some("parent"), child)
      .build()
      .expect("a valid skeleton")
  }

  fn heading(world: Affine2) -> f32 {
    world.matrix2.x_axis.to_angle()
  }

  fn close(actual: f32, expected: f32) {
    assert!(
      (actual - expected).abs() < 1e-4,
      "expected {expected}, got {actual}"
    );
  }

  #[test]
  fn a_world_rotation_points_the_bone_under_every_inherit_mode() {
    // The entity is turned, stretched and mirrored, the parent bone turned
    // and stretched: whatever the child inherits, its x axis ends up at the
    // world angle asked for
    let root =
      Affine2::from_scale_angle_translation(Vec2::new(-1.5, 2.0), 0.4, Vec2::new(7.0, 3.0));
    for inherit in [
      Inherit::Normal,
      Inherit::OnlyTranslation,
      Inherit::NoRotationOrReflection,
      Inherit::NoScale,
      Inherit::NoScaleOrReflection,
    ] {
      for angle in [0.0, 1.0, -2.5, 3.0] {
        let aimed = BoneOverride {
          rotation: Some(angle),
          space: Space::World,
          ..BoneOverride::default()
        };
        let skeleton = posed(pair(inherit), root, |skeleton| {
          skeleton.override_bone("child", aimed);
        });
        let child = skeleton.bone_world("child").expect("the child");
        let turn = shortest(heading(child) - angle);
        assert!(turn.abs() < 1e-4, "{inherit:?} at {angle}: off by {turn}");
      }
    }
  }

  #[test]
  fn a_world_rotation_points_a_bone_whose_own_x_scale_is_negative() {
    let data = pair_with(BoneSetup {
      x: 5.0,
      rotation: 0.3,
      scale_x: -1.0,
      shear_x: 0.2,
      ..BoneSetup::default()
    });
    let aimed = BoneOverride {
      rotation: Some(1.0),
      space: Space::World,
      ..BoneOverride::default()
    };
    let skeleton = posed(data, Affine2::from_angle(0.5), |skeleton| {
      skeleton.override_bone("child", aimed);
    });
    close(
      heading(skeleton.bone_world("child").expect("the child")),
      1.0,
    );
  }

  #[test]
  fn a_world_translation_puts_a_bone_at_a_point_and_its_children_follow() {
    let root =
      Affine2::from_scale_angle_translation(Vec2::new(2.0, 2.0), -0.6, Vec2::new(7.0, 3.0));
    let moved = BoneOverride {
      translation: Some(Vec2::new(-4.0, 9.0)),
      space: Space::World,
      ..BoneOverride::default()
    };
    // The parent's override is applied before the child's is converted
    // through it: both end where asked
    let grabbed = BoneOverride {
      translation: Some(Vec2::new(20.0, 20.0)),
      ..moved
    };
    let skeleton = posed(pair(Inherit::Normal), root, |skeleton| {
      skeleton.override_bone("child", grabbed);
      skeleton.override_bone("parent", moved);
    });
    let parent = skeleton.bone_world("parent").expect("the parent");
    assert!(parent.translation.abs_diff_eq(Vec2::new(-4.0, 9.0), 1e-4));
    let child = skeleton.bone_world("child").expect("the child");
    assert!(
      child.translation.abs_diff_eq(Vec2::new(20.0, 20.0), 1e-4),
      "{child:?}"
    );
  }

  #[test]
  fn an_override_blends_from_the_animated_value_by_its_mix() {
    let half = BoneOverride {
      rotation: Some(0.3 + 1.0),
      translation: Some(Vec2::new(9.0, 4.0)),
      scale: Some(Vec2::new(3.0, 5.0)),
      mix: 0.5,
      ..BoneOverride::default()
    };
    let skeleton = posed(pair(Inherit::Normal), Affine2::IDENTITY, |skeleton| {
      skeleton.override_bone("child", half);
    });
    // Half way from the setup pose (5, 0), 0.3 and (1, 1)
    let child = skeleton.pose.bones[1];
    close(child.x, 7.0);
    close(child.y, 2.0);
    close(child.rotation, 0.8);
    close(child.scale_x, 2.0);
    close(child.scale_y, 3.0);
    // A rotation blends the short way round: from 0.3 to just under a full
    // turn more is a small turn back
    let round = BoneOverride {
      rotation: Some(0.3 + std::f32::consts::TAU - 0.2),
      mix: 0.5,
      ..BoneOverride::default()
    };
    let skeleton = posed(pair(Inherit::Normal), Affine2::IDENTITY, |skeleton| {
      skeleton.override_bone("child", round);
    });
    close(skeleton.pose.bones[1].rotation, 0.2);
    // A mix outside 0 to 1 is clamped
    let over = BoneOverride { mix: 7.0, ..half };
    let skeleton = posed(pair(Inherit::Normal), Affine2::IDENTITY, |skeleton| {
      skeleton.override_bone("child", over);
    });
    close(skeleton.pose.bones[1].x, 9.0);
  }

  #[test]
  fn a_world_override_through_a_flat_parent_is_left_out() {
    // A zero scale above the bone: no local value puts it at a world point
    // or angle, so the animated ones stay, and nothing is NaN
    let aimed = BoneOverride {
      rotation: Some(1.0),
      translation: Some(Vec2::new(3.0, 3.0)),
      space: Space::World,
      ..BoneOverride::default()
    };
    let flat = Affine2::from_scale(Vec2::new(0.0, 1.0));
    let skeleton = posed(pair(Inherit::Normal), flat, |skeleton| {
      skeleton.override_bone("child", aimed);
    });
    let child = skeleton.pose.bones[1];
    assert_eq!((child.x, child.y, child.rotation), (5.0, 0.0, 0.3));
    assert!(skeleton.pose.world.iter().all(|world| world.is_finite()));
  }

  #[test]
  fn values_that_are_not_finite_are_taken_out_with_one_warning() {
    let log = capture_log();
    let broken = BoneOverride {
      rotation: Some(f32::INFINITY),
      translation: Some(Vec2::new(f32::NAN, 1.0)),
      scale: Some(Vec2::new(2.0, 3.0)),
      mix: 1.0,
      ..BoneOverride::default()
    };
    let unmixed = BoneOverride {
      scale: Some(Vec2::new(9.0, 9.0)),
      mix: f32::NAN,
      ..BoneOverride::default()
    };
    let skeleton = posed(pair(Inherit::Normal), Affine2::IDENTITY, |skeleton| {
      skeleton.override_bone("child", broken);
      skeleton.override_bone("child", broken);
      skeleton.override_bone("parent", unmixed);
    });
    // The scale given applies; the rest is the animated pose
    let child = skeleton.pose.bones[1];
    assert_eq!((child.x, child.y, child.rotation), (5.0, 0.0, 0.3));
    assert_eq!((child.scale_x, child.scale_y), (2.0, 3.0));
    // A mix that is not finite is 0: the override does nothing
    assert_eq!(skeleton.pose.bones[0].scale_x, 2.0);
    assert_eq!(log.containing("not finite"), 1, "{:?}", log.warnings());
  }

  #[test]
  fn names_the_data_lacks_warn_once_each_up_to_a_bound() {
    let log = capture_log();
    let skeleton = posed(pair(Inherit::Normal), Affine2::IDENTITY, |skeleton| {
      // Kept until the data lands, then found missing: one warning a name
      // and kind, however many times each was set
      for _ in 0..3 {
        skeleton.set_skin("gold");
        skeleton.set_attachment("gold", Some("ring"));
        skeleton.override_bone("gold", BoneOverride::default());
      }
    });
    assert_eq!(log.containing("\"gold\""), 3, "{:?}", log.warnings());
    // Asked again once the data has landed, they say nothing more, and
    // nothing is kept for them
    let mut skeleton = skeleton;
    skeleton.set_skin("gold");
    skeleton.override_bone("gold", BoneOverride::default());
    skeleton.override_bone("silver", BoneOverride::default());
    assert_eq!(log.containing("\"gold\""), 3);
    assert_eq!(log.containing("\"silver\""), 1);
    assert_eq!(skeleton.overrides.len(), 1);
    // A game that makes names up without end is warned about the first of
    // them only, and the skeleton remembers no more
    for made_up in 0..200 {
      skeleton.override_bone(&format!("bone {made_up}"), BoneOverride::default());
    }
    assert_eq!(skeleton.warned.len(), MAX_WARNED);
    assert_eq!(skeleton.overrides.len(), 1);
  }

  /// The child of [`pair`] turned to `turn` by an override
  fn turn_child(skeleton: &mut Skeleton, turn: f32) {
    let turned = BoneOverride {
      rotation: Some(turn),
      ..BoneOverride::default()
    };
    skeleton.override_bone("child", turned);
  }

  #[test]
  fn the_last_two_steps_transforms_are_kept_by_bone_name_across_new_data() {
    let mut world = asset_world();
    let handle = world
      .resource_mut::<Assets<SkeletonData>>()
      .add(pair(Inherit::Normal));
    let mut skeleton = Skeleton::new(handle.clone());
    // Each step turns the child to another angle, under an entity that
    // has moved: the transforms kept are in the skeleton's own space, so
    // the entity's move is no part of them
    let step = |world: &World, skeleton: &mut Skeleton, turn: f32| {
      let assets = world.resource::<Assets<SkeletonData>>();
      let data = assets.get(&handle).expect("the data");
      let seen = (handle.id(), assets.version(&handle).expect("a version"));
      turn_child(skeleton, turn);
      let root = Affine2::from_translation(Vec2::new(turn * 100.0, 0.0));
      let fired = &mut Fired::default();
      skeleton.step(data, seen, None, 1.0 / 60.0, root.into(), fired);
    };
    // A first step has nothing before it: each bone's previous transform
    // is its current one
    step(&world, &mut skeleton, 1.0);
    let first = skeleton.pose.skeleton_space.clone();
    assert_eq!(skeleton.previous_space, first);
    // From then on the previous ones are the last step's
    step(&world, &mut skeleton, 2.0);
    let second = skeleton.pose.skeleton_space.clone();
    assert_eq!(skeleton.previous_space, first);
    assert_ne!(second[1], first[1], "the child turned");
    assert_eq!(second[0], first[0], "the parent stood still in its space");
    step(&world, &mut skeleton, 3.0);
    assert_eq!(skeleton.previous_space, second);
    let third = skeleton.pose.skeleton_space.clone();
    // New data with a bone added first, so every bone's place moves: a
    // bone it had before keeps its last transform as its previous one, by
    // name, and the new bone's is where it now is
    let longer = SkeletonData::builder()
      .bone("tail", None, BoneSetup::default())
      .bone("parent", None, BoneSetup::default())
      .bone("child", Some("parent"), BoneSetup::default())
      .build()
      .expect("a valid skeleton");
    *world
      .resource_mut::<Assets<SkeletonData>>()
      .get_mut(&handle)
      .expect("the data") = longer;
    step(&world, &mut skeleton, 4.0);
    let now = &skeleton.pose.skeleton_space;
    assert_eq!(now.len(), 3);
    assert_eq!(skeleton.previous_space[1], third[0]);
    assert_eq!(skeleton.previous_space[2], third[1]);
    assert_eq!(skeleton.previous_space[0], now[0]);
    assert_eq!(skeleton.pose.world[0].translation, Vec2::new(400.0, 0.0));
  }

  #[test]
  fn a_step_after_a_missed_one_and_a_pose_outside_a_step_are_first_placings() {
    let mut world = asset_world();
    let handle = world
      .resource_mut::<Assets<SkeletonData>>()
      .add(pair(Inherit::Normal));
    let assets = world.resource::<Assets<SkeletonData>>();
    let data = assets.get(&handle).expect("the data");
    let seen = (handle.id(), assets.version(&handle).expect("a version"));
    let mut skeleton = Skeleton::new(handle.clone());
    let step = |skeleton: &mut Skeleton, turn: f32, number: u64| {
      turn_child(skeleton, turn);
      let (root, fired) = (Affine2::IDENTITY, &mut Fired::default());
      skeleton.step(data, seen, None, 1.0 / 60.0, root.into(), fired);
      skeleton.took_step(number);
    };
    // Steps 7 and 8 follow each other: the second keeps the first's
    // transforms to be drawn from
    step(&mut skeleton, 1.0, 7);
    let first = skeleton.pose.skeleton_space.clone();
    step(&mut skeleton, 2.0, 8);
    assert_eq!(skeleton.previous_space, first);
    assert_eq!(skeleton.stepped(), 8);
    // It missed steps 9 to 11 (disabled, or held): step 12 has no pose of
    // step 11 to be drawn from, so it is drawn where it is
    step(&mut skeleton, 3.0, 12);
    assert_ne!(skeleton.pose.skeleton_space, first);
    assert_eq!(skeleton.previous_space, skeleton.pose.skeleton_space);
    // And step 13 follows on again
    let twelfth = skeleton.pose.skeleton_space.clone();
    step(&mut skeleton, 4.0, 13);
    assert_eq!(skeleton.previous_space, twelfth);

    // A pose made outside a step (a reload landing while the clock is
    // stopped) is drawn as it is, though its bones were carried by name;
    // and the step after it follows on from it
    turn_child(&mut skeleton, 5.0);
    let newer = (seen.0, seen.1 + 1);
    skeleton.appear(data, newer, None, Affine2::IDENTITY.into());
    skeleton.posed_after(13);
    let appeared = skeleton.pose.skeleton_space.clone();
    assert_eq!(skeleton.previous_space, appeared);
    turn_child(&mut skeleton, 6.0);
    let (root, fired) = (Affine2::IDENTITY, &mut Fired::default());
    skeleton.step(data, newer, None, 1.0 / 60.0, root.into(), fired);
    skeleton.took_step(14);
    assert_eq!(skeleton.previous_space, appeared);
    assert_ne!(skeleton.pose.skeleton_space, appeared);
  }

  #[test]
  fn a_flipped_skeleton_s_bones_mirror_about_its_entity_s_origin() {
    // An entity at (100, 50) turned a quarter turn: its x axis points down
    // the screen. The child of the pair is at (10, 10) in the skeleton's
    // space (5 along its parent, which points down from (10, 0) at twice
    // the length)
    let root = Affine2::from_angle_translation(FRAC_PI_2, Vec2::new(100.0, 50.0));
    let flipped = |x: bool, y: bool| {
      let stand = Stand {
        root,
        flip: Flip { x, y },
      };
      let mut world = asset_world();
      let data = pair(Inherit::Normal);
      let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
      let assets = world.resource::<Assets<SkeletonData>>();
      let data = assets.get(&handle).expect("the data");
      let seen = (handle.id(), assets.version(&handle).expect("a version"));
      let mut skeleton = Skeleton::new(handle.clone());
      let fired = &mut Fired::default();
      skeleton.step(data, seen, None, 1.0 / 60.0, stand, fired);
      skeleton
    };
    let child = BoneId(1);
    let plain = flipped(false, false);
    let space = plain.bone_space(child).expect("the child");
    same_point(space.translation, Vec2::new(10.0, 10.0));
    // Unflipped: (10, 10) of the skeleton is 10 down and 10 left of the
    // entity
    let world = plain.bone_world("child").expect("the child");
    same_point(world.translation, Vec2::new(90.0, 60.0));

    // Flipped along the skeleton's x, the bone is at (-10, 10) of it:
    // mirrored about the entity's origin in the entity's own frame,
    // wherever the entity points
    let mirrored = flipped(true, false);
    assert_eq!(mirrored.flip(), Flip { x: true, y: false });
    let space = mirrored.bone_space(child).expect("the child");
    same_point(space.translation, Vec2::new(-10.0, 10.0));
    let world = mirrored.bone_world("child").expect("the child");
    same_point(world.translation, Vec2::new(90.0, 40.0));
    assert!(world.matrix2.determinant() * plain_det(&plain) < 0.0);
    // What the skeleton keeps to be drawn from is not mirrored: the drawer
    // mirrors it
    assert_eq!(
      mirrored.pose.skeleton_space, plain.pose.skeleton_space,
      "the skeleton's own space is the same"
    );

    let both = flipped(true, true);
    let world = both.bone_world("child").expect("the child");
    same_point(world.translation, Vec2::new(110.0, 40.0));
    assert!(world.matrix2.determinant() * plain_det(&plain) > 0.0);
  }

  fn plain_det(skeleton: &Skeleton) -> f32 {
    let world = skeleton.bone_world("child").expect("the child");
    world.matrix2.determinant()
  }

  fn same_point(actual: Vec2, expected: Vec2) {
    assert!(
      actual.abs_diff_eq(expected, 1e-4),
      "expected {expected}, got {actual}"
    );
  }

  #[test]
  fn a_world_override_still_aims_at_its_point_on_a_flipped_skeleton() {
    // The child is aimed at a point in the world, and its parent's origin
    // put on another: both hold whichever way the skeleton is flipped
    let root = Affine2::from_angle_translation(0.4, Vec2::new(100.0, 50.0));
    for (x, y) in [(false, false), (true, false), (false, true), (true, true)] {
      let mut world = asset_world();
      let data = pair(Inherit::Normal);
      let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
      let assets = world.resource::<Assets<SkeletonData>>();
      let data = assets.get(&handle).expect("the data");
      let seen = (handle.id(), assets.version(&handle).expect("a version"));
      let mut skeleton = Skeleton::new(handle.clone());
      let placed = BoneOverride {
        translation: Some(Vec2::new(120.0, 40.0)),
        space: Space::World,
        ..BoneOverride::default()
      };
      let aimed = BoneOverride {
        rotation: Some(1.1),
        space: Space::World,
        ..BoneOverride::default()
      };
      skeleton.override_bone("parent", placed);
      skeleton.override_bone("child", aimed);
      let stand = Stand {
        root,
        flip: Flip { x, y },
      };
      let fired = &mut Fired::default();
      skeleton.step(data, seen, None, 1.0 / 60.0, stand, fired);
      let parent = skeleton.bone_world("parent").expect("the parent");
      same_point(parent.translation, Vec2::new(120.0, 40.0));
      let child = skeleton.bone_world("child").expect("the child");
      let points = child.matrix2.x_axis.to_angle();
      assert!((points - 1.1).abs() < 1e-4, "flipped {x} {y}: {points}");
    }
  }

  #[test]
  fn what_is_set_before_the_data_lands_is_bounded() {
    // Before the data lands no name can be checked, so a game that sets
    // made-up names without end is kept to as many as a skeleton has bones
    // and slots, with one warning
    let log = capture_log();
    let mut world = asset_world();
    let data = pair(Inherit::Normal);
    let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
    let mut skeleton = Skeleton::new(handle);
    for made_up in 0..300 {
      let name = format!("part {made_up}");
      skeleton.override_bone(&name, BoneOverride::default());
      skeleton.set_attachment(&name, None);
    }
    assert_eq!(skeleton.overrides.len(), MAX_BONES);
    assert_eq!(skeleton.attachments.len(), MAX_SLOTS);
    assert_eq!(log.containing("more are ignored"), 1);
    // One already set can still be changed, or cleared to make room
    let turned = BoneOverride {
      rotation: Some(1.0),
      ..BoneOverride::default()
    };
    skeleton.override_bone("part 0", turned);
    assert_eq!(skeleton.overrides[0].value.rotation, Some(1.0));
    skeleton.clear_override("part 1");
    skeleton.override_bone("another", turned);
    assert_eq!(skeleton.overrides.len(), MAX_BONES);
  }

  #[test]
  fn a_slot_showing_a_mesh_answers_as_any_slot_and_is_no_point_or_box() {
    use crate::core::graphics::Image;
    use crate::core::skeleton::{BoundingBoxAttachment, MeshAttachment, MeshVertices, Region};
    // Three slots that each show an attachment called "mark": a mesh, then
    // a point, then a bounding box
    let mut world = asset_world();
    let image = world
      .resource_mut::<Assets<Image>>()
      .add(Image::solid_rect(4, 4, 255, 255, 255, 255));
    let shown = || SlotSetup {
      attachment: Some(Arc::from("mark")),
      ..SlotSetup::default()
    };
    let corners = vec![Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(0.0, 4.0)];
    let mesh = MeshAttachment::new(
      0,
      MeshVertices::Unweighted(corners.clone()),
      vec![Vec2::ZERO, Vec2::X, Vec2::Y],
      vec![0, 1, 2],
    );
    let point = PointAttachment {
      x: 3.0,
      y: 4.0,
      rotation: 0.0,
    };
    let data = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .slot("cape", "root", shown())
      .slot("aim", "root", shown())
      .slot("hit", "root", shown())
      .region(Region {
        name: Arc::from("cloth"),
        image,
        x: 0,
        y: 0,
        width: 4,
        height: 4,
      })
      .attachment("default", "cape", "mark", Attachment::Mesh(mesh))
      .attachment("default", "aim", "mark", Attachment::Point(point))
      .attachment(
        "default",
        "hit",
        "mark",
        Attachment::BoundingBox(BoundingBoxAttachment {
          vertices: corners.clone(),
        }),
      )
      .build()
      .expect("a valid skeleton");
    let root = Affine2::from_translation(Vec2::new(10.0, 20.0));
    let mut skeleton = posed(data.clone(), root, |_| {});
    // The slot says what it shows, a mesh as anything else
    assert_eq!(skeleton.attachment("cape"), Some("mark"));
    // The mesh is neither a point nor a bounding box: those of its name
    // are found in the slots after it
    assert_eq!(skeleton.point("mark"), Some((Vec2::new(13.0, 24.0), 0.0)));
    let mut out = Vec::new();
    assert!(skeleton.bounding_box("mark", &mut out));
    let moved: Vec<Vec2> = corners.iter().map(|&at| at + root.translation).collect();
    assert_eq!(out, moved);
    // With those two hidden only the mesh has the name: no point, no box
    skeleton.set_attachment("aim", None);
    skeleton.set_attachment("hit", None);
    let fired = &mut Fired::default();
    let seen = skeleton.seen().expect("a pose");
    skeleton.step(&data, seen, None, 1.0 / 60.0, root.into(), fired);
    assert_eq!(skeleton.attachment("cape"), Some("mark"));
    assert_eq!(skeleton.point("mark"), None);
    assert!(!skeleton.bounding_box("mark", &mut out) && out.is_empty());
    // The game hides the mesh's slot as it hides any
    skeleton.set_attachment("cape", None);
    skeleton.step(&data, seen, None, 1.0 / 60.0, root.into(), fired);
    assert_eq!(skeleton.attachment("cape"), None);
  }

  /// A root with a slot "hand" showing "open" and a bare slot "hat". The
  /// default skin has "open" for the hand; the skin "gloves" has its own
  /// "open" there and alone has "fist"; the skin "hats" has "cap" for the
  /// hat. The animation "grip" keys the hand to "fist"
  fn dressed() -> SkeletonData {
    let point = || Attachment::Point(PointAttachment::default());
    let open = SlotSetup {
      attachment: Some(Arc::from("open")),
      ..SlotSetup::default()
    };
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .slot("hand", "root", open)
      .slot("hat", "root", SlotSetup::default())
      .skin("gloves")
      .skin("hats")
      .attachment("default", "hand", "open", point())
      .attachment("gloves", "hand", "open", point())
      .attachment("gloves", "hand", "fist", point())
      .attachment("hats", "hat", "cap", point())
      .animation(Animation {
        name: Arc::from("grip"),
        duration: 1.0,
        timelines: vec![Timeline::Attachment {
          slot: SlotId(0),
          frames: vec![(0.0, Some(Arc::from("fist")))],
        }],
      })
      .build()
      .expect("a valid skeleton")
  }

  #[test]
  fn a_skeleton_says_what_it_wears_and_what_each_slot_shows() {
    let mut world = asset_world();
    let handle = world.resource_mut::<Assets<SkeletonData>>().add(dressed());
    let assets = world.resource::<Assets<SkeletonData>>();
    let data = assets.get(&handle).expect("the data");
    let seen = (handle.id(), assets.version(&handle).expect("a version"));
    let mut skeleton = Skeleton::new(handle.clone());
    let step = |skeleton: &mut Skeleton, animation: Option<&mut SkeletonAnimation>| {
      let fired = &mut Fired::default();
      let stand = Affine2::IDENTITY.into();
      skeleton.step(data, seen, animation, 1.0 / 60.0, stand, fired);
    };
    fn worn(skeleton: &Skeleton) -> Vec<&str> {
      skeleton.skins().collect()
    }
    // Before the data lands it wears nothing, and no slot shows anything
    assert!(worn(&skeleton).is_empty());
    assert_eq!(skeleton.attachment("hand"), None);
    // The skins a call asks for meanwhile are kept by name, unchecked, and
    // read back as kept
    skeleton.set_skins(&["gloves", "gold"]);
    assert_eq!(worn(&skeleton), ["gloves", "gold"]);
    assert_eq!(skeleton.attachment("hand"), None);
    // The data lands: the skin it lacks is not worn, and each slot shows
    // its setup attachment (a bare slot nothing, as a slot it lacks)
    step(&mut skeleton, None);
    assert_eq!(worn(&skeleton), ["gloves"]);
    assert_eq!(skeleton.attachment("hand"), Some("open"));
    assert_eq!(skeleton.attachment("hat"), None);
    assert_eq!(skeleton.attachment("tail"), None);
    // What the game sets on a slot reads back from the skeleton's next
    // step: until then the answer is the last step's
    skeleton.set_attachment("hand", Some("fist"));
    assert_eq!(skeleton.attachment("hand"), Some("open"));
    step(&mut skeleton, None);
    assert_eq!(skeleton.attachment("hand"), Some("fist"));
    skeleton.set_attachment("hand", None);
    step(&mut skeleton, None);
    assert_eq!(skeleton.attachment("hand"), None);
    // Handed back, the slot shows what an animation keys there, and with
    // no animation its setup attachment again
    skeleton.clear_attachment("hand");
    let mut animation = SkeletonAnimation::new();
    animation.set_animation(0, "grip", true);
    step(&mut skeleton, Some(&mut animation));
    assert_eq!(skeleton.attachment("hand"), Some("fist"));
    step(&mut skeleton, None);
    assert_eq!(skeleton.attachment("hand"), Some("open"));
    // Skins read back at once, in the order worn
    skeleton.set_skins(&["hats", "gloves"]);
    assert_eq!(worn(&skeleton), ["hats", "gloves"]);
    skeleton.set_attachment("hat", Some("cap"));
    step(&mut skeleton, None);
    assert_eq!(skeleton.attachment("hat"), Some("cap"));
    // A name no skin worn has in the slot shows nothing: the cap is the
    // hats' own, and the fist the gloves'. The skins are looked up when
    // the skeleton is asked, so the answer changes at once, with no step
    skeleton.set_skin("gloves");
    assert_eq!(worn(&skeleton), ["gloves"]);
    assert_eq!(skeleton.attachment("hat"), None);
    skeleton.set_skin("hats");
    assert_eq!(skeleton.attachment("hat"), Some("cap"));
    skeleton.set_skin("gloves");
    step(&mut skeleton, None);
    assert_eq!(skeleton.attachment("hat"), None);
    skeleton.set_attachment("hand", Some("fist"));
    step(&mut skeleton, None);
    assert_eq!(skeleton.attachment("hand"), Some("fist"));
    skeleton.set_skins(&[]);
    assert!(worn(&skeleton).is_empty());
    step(&mut skeleton, None);
    assert_eq!(skeleton.attachment("hand"), None);
    // Once the data is known, a skin it lacks is left out of what is worn
    skeleton.set_skins(&["hats", "gold"]);
    assert_eq!(worn(&skeleton), ["hats"]);
  }

  const KEY: [f32; 6] = [2.0, -4.0, 6.0, 8.0, -10.0, 12.0];
  const NEXT: [f32; 6] = [4.0, 0.0, 6.0, -8.0, 10.0, 0.0];

  /// A root bone and a slot "banner" that shows "flag". The default skin
  /// has "flag" and "pennant", each a mesh of three positions, and "badge",
  /// a region; the skin "wide" has a "flag" of four positions. The
  /// animations are named for what they do, and no mix takes time
  fn banner() -> SkeletonData {
    use crate::core::skeleton::{Curve, MeshAttachment, MeshVertices, Region, RegionAttachment};
    let mesh = |corners: usize| {
      let at: Vec<Vec2> = (0..corners)
        .map(|corner| Vec2::new(corner as f32, (corner * corner) as f32))
        .collect();
      let triangles = (1..corners as u16 - 1)
        .flat_map(|corner| [0, corner, corner + 1])
        .collect();
      Attachment::Mesh(MeshAttachment::new(
        0,
        MeshVertices::Unweighted(at.clone()),
        at,
        triangles,
      ))
    };
    let deform = |attachment: &str, frames: Vec<(f32, Vec<f32>)>| Timeline::Deform {
      slot: SlotId(0),
      attachment: Arc::from(attachment),
      frames: frames
        .into_iter()
        .map(|(time, offsets)| (time, offsets, Curve::Linear))
        .collect(),
    };
    let clip = |name: &str, timelines: Vec<Timeline>| Animation {
      name: Arc::from(name),
      duration: 1.0,
      timelines,
    };
    let swap = Timeline::Attachment {
      slot: SlotId(0),
      frames: vec![
        (0.0, Some(Arc::from("flag"))),
        (0.5, Some(Arc::from("pennant"))),
      ],
    };
    let skin = SkeletonData::DEFAULT_SKIN;
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .slot(
        "banner",
        "root",
        SlotSetup {
          attachment: Some(Arc::from("flag")),
          ..SlotSetup::default()
        },
      )
      .region(Region {
        name: Arc::from("cloth"),
        image: Handle::detached(),
        x: 0,
        y: 0,
        width: 4,
        height: 4,
      })
      .skin("wide")
      .attachment(skin, "banner", "flag", mesh(3))
      .attachment(skin, "banner", "pennant", mesh(3))
      .attachment(
        skin,
        "banner",
        "badge",
        Attachment::Region(RegionAttachment::new(0, 4.0, 4.0)),
      )
      .attachment("wide", "banner", "flag", mesh(4))
      // The flag held in a shape, and growing into it over a second
      .animation(clip(
        "wave",
        vec![deform("flag", vec![(0.0, KEY.to_vec())])],
      ))
      .animation(clip(
        "grow",
        vec![deform("flag", vec![(0.0, Vec::new()), (1.0, KEY.to_vec())])],
      ))
      // The pennant held in another
      .animation(clip(
        "furl",
        vec![deform("pennant", vec![(0.0, NEXT.to_vec())])],
      ))
      // The flag in its shape, then from half way the pennant in its own
      .animation(clip(
        "both",
        vec![
          swap,
          deform("flag", vec![(0.0, KEY.to_vec())]),
          deform("pennant", vec![(0.0, NEXT.to_vec())]),
        ],
      ))
      .build()
      .expect("a valid skeleton")
  }

  /// A skeleton and its animation state, over data in a store, stepped by
  /// hand
  struct Played {
    world: World,
    skeleton: Skeleton,
    animation: SkeletonAnimation,
  }

  impl Played {
    fn new(data: SkeletonData) -> Self {
      let mut world = asset_world();
      let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
      Self {
        world,
        skeleton: Skeleton::new(handle),
        animation: SkeletonAnimation::new(),
      }
    }

    fn play(&mut self, name: &str, looping: bool) {
      let entry = self.animation.set_animation(0, name, looping);
      assert!(entry.is_some() || self.skeleton.seen().is_none(), "{name}");
    }

    fn step(&mut self, dt: f32) {
      let assets = self.world.resource::<Assets<SkeletonData>>();
      let handle = self.skeleton.data().clone();
      let data = assets.get(&handle).expect("the data");
      let seen = (handle.id(), assets.version(&handle).expect("a version"));
      let (stand, fired) = (Affine2::IDENTITY.into(), &mut Fired::default());
      let animation = Some(&mut self.animation);
      self.skeleton.step(data, seen, animation, dt, stand, fired);
    }

    /// Puts other data in the store, as a reload does
    fn reload(&mut self, data: SkeletonData) {
      let handle = self.skeleton.data().clone();
      let mut assets = self.world.resource_mut::<Assets<SkeletonData>>();
      *assets.get_mut(&handle).expect("the data") = data;
    }

    /// The banner slot's offsets of the step before the last and of the
    /// last, each empty for none
    fn lists(&self) -> (&[f32], &[f32]) {
      match self.skeleton.pose.deforms.first() {
        Some(deform) => (&deform.previous, &deform.offsets),
        None => (&[], &[]),
      }
    }
  }

  fn scaled(key: [f32; 6], by: f32) -> Vec<f32> {
    key.iter().map(|value| value * by).collect()
  }

  fn close_lists(actual: (&[f32], &[f32]), expected: (&[f32], &[f32])) {
    let near = |got: &[f32], want: &[f32]| {
      got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-4)
    };
    assert!(
      near(actual.0, expected.0) && near(actual.1, expected.1),
      "expected {expected:?}, got {actual:?}"
    );
  }

  #[test]
  fn a_deform_resets_when_the_slot_s_attachment_changes() {
    let log = capture_log();
    let mut played = Played::new(banner());
    // While the slot shows one attachment, each step's offsets are kept
    // for the next to be drawn from
    played.play("grow", false);
    played.step(0.25);
    played.step(0.25);
    close_lists(played.lists(), (&scaled(KEY, 0.25), &scaled(KEY, 0.5)));
    played.play("both", false);
    played.step(0.25);
    close_lists(played.lists(), (&scaled(KEY, 0.5), &KEY));
    // An attachment key shows the pennant, which has a deform of its own
    // and as many positions as the flag. It takes its own shape at once:
    // the flag's offsets are not the pennant's to be drawn from
    played.step(0.5);
    assert_eq!(played.skeleton.attachment("banner"), Some("pennant"));
    close_lists(played.lists(), (&NEXT, &NEXT));
    played.step(0.1);
    close_lists(played.lists(), (&NEXT, &NEXT));
    // "both" keys a deform for each name the slot shows in turn, and the
    // slot shows one of them at a time: the other is skipped in silence
    assert_eq!(log.count(log::Level::Warn), 0, "{:?}", log.warnings());
    // Back to the flag, by an animation that keys no attachment
    played.play("wave", true);
    played.step(0.1);
    assert_eq!(played.skeleton.attachment("banner"), Some("flag"));
    close_lists(played.lists(), (&KEY, &KEY));
    // The game shows the pennant, whose deform nothing playing keys: its
    // offsets are none, the setup shape, in that step and to be drawn from
    played.skeleton.set_attachment("banner", Some("pennant"));
    played.step(0.1);
    close_lists(played.lists(), (&[], &[]));
    played.step(0.1);
    close_lists(played.lists(), (&[], &[]));
    // Hidden, then handed back to the animation: the flag has its key's
    // shape at once, drawn from nothing else
    played.skeleton.set_attachment("banner", None);
    played.step(0.1);
    close_lists(played.lists(), (&[], &[]));
    played.skeleton.clear_attachment("banner");
    played.step(0.1);
    close_lists(played.lists(), (&KEY, &KEY));
    // A pose made outside a step is drawn as it is, too
    played.play("grow", false);
    played.step(0.5);
    close_lists(played.lists(), (&KEY, &scaled(KEY, 0.5)));
    let assets = played.world.resource::<Assets<SkeletonData>>();
    let handle = played.skeleton.data().clone();
    let data = assets.get(&handle).expect("the data");
    let newer = (handle.id(), assets.version(&handle).expect("a version") + 1);
    let animation = Some(&mut played.animation);
    played
      .skeleton
      .appear(data, newer, animation, Affine2::IDENTITY.into());
    close_lists(played.lists(), (&scaled(KEY, 0.5), &scaled(KEY, 0.5)));
    // Nothing was said of any of it: a slot the game or a key gives
    // another attachment, or hides, under a deform of the one before
    assert_eq!(log.count(log::Level::Warn), 0, "{:?}", log.warnings());
  }

  #[test]
  fn a_deform_moves_what_its_slot_shows_once_the_game_and_the_skins_have_their_say() {
    let log = capture_log();
    let skipped = |log: &crate::testing::LogCapture| log.containing("the deform is skipped");
    // "furl" keys the pennant's deform, and the slot's setup attachment is
    // the flag: skipped, in silence
    let mut played = Played::new(banner());
    played.play("furl", true);
    played.step(0.1);
    close_lists(played.lists(), (&[], &[]));
    assert_eq!(skipped(&log), 0, "{:?}", log.warnings());
    // The game shows the pennant in the slot. What the game sets is
    // written after the animation, and the deform is judged after that
    played.skeleton.set_attachment("banner", Some("pennant"));
    played.step(0.1);
    close_lists(played.lists(), (&NEXT, &NEXT));
    played.skeleton.clear_attachment("banner");
    played.step(0.1);
    close_lists(played.lists(), (&[], &[]));

    // The mesh a deform must fit is the one the skins worn show under its
    // name: the skin "wide" has a flag of four positions, and "wave" keys
    // three
    let mut played = Played::new(banner());
    played.play("wave", true);
    played.step(0.1);
    close_lists(played.lists(), (&KEY, &KEY));
    for _ in 0..3 {
      played.skeleton.set_skin("wide");
      played.step(0.1);
      played.step(0.1);
      close_lists(played.lists(), (&[], &[]));
      played.skeleton.set_skins(&[]);
      played.step(0.1);
      assert_eq!(played.lists().1, KEY);
    }
    // One warning for the animation, however often the skin comes and
    // goes; and none for the pennant's deform under the flag, above
    assert_eq!(skipped(&log), 1, "{:?}", log.warnings());
    assert_eq!(log.containing("the mesh has 4"), 1, "{:?}", log.warnings());
    // With no animation state there are no deform keys
    let mut bare = posed(banner(), Affine2::IDENTITY, |_| {});
    assert!(bare.pose.deforms.is_empty());
    bare.settle();
  }

  #[test]
  fn a_steady_step_takes_no_more_room_for_its_deforms() {
    let mut played = Played::new(banner());
    played.play("grow", true);
    for _ in 0..4 {
      played.step(0.1);
    }
    // The slot's two lists change places each step, and neither moves nor
    // grows, through a crossfade and a settle as through plain steps
    let rooms = |played: &Played| {
      let deforms = &played.skeleton.pose.deforms;
      let deform = &deforms[0];
      let mut lists = [
        (deform.offsets.as_ptr(), deform.offsets.capacity()),
        (deform.previous.as_ptr(), deform.previous.capacity()),
      ];
      lists.sort();
      (deforms.as_ptr(), deforms.capacity(), lists)
    };
    let before = rooms(&played);
    for step in 0..40 {
      if step % 10 == 5 {
        let name = if step % 20 == 5 { "wave" } else { "grow" };
        let entry = played.animation.set_animation(0, name, true);
        let entry = entry.and_then(|id| played.animation.entry_mut(id));
        entry.expect("a live entry").mix_duration = 0.25;
      }
      if step == 30 {
        played.skeleton.settle();
      }
      played.step(0.1);
      assert_eq!(rooms(&played), before, "step {step}");
      assert_eq!(played.lists().1.len(), 6, "step {step}");
    }
  }

  #[test]
  fn a_first_placing_draws_its_deform_as_it_is() {
    let mut played = Played::new(banner());
    played.play("grow", false);
    let step = |played: &mut Played, number: u64| {
      played.step(0.125);
      played.skeleton.took_step(number);
    };
    // Steps 7 and 8 follow each other: the second keeps the first's
    // offsets to be drawn from
    step(&mut played, 7);
    step(&mut played, 8);
    close_lists(played.lists(), (&scaled(KEY, 0.125), &scaled(KEY, 0.25)));
    // It missed steps 9 to 11 (disabled, or held): step 12 has no shape of
    // step 11 to be drawn from, and its mesh is drawn in the shape it has,
    // as its bones are drawn where they are
    step(&mut played, 12);
    close_lists(played.lists(), (&scaled(KEY, 0.375), &scaled(KEY, 0.375)));
    step(&mut played, 13);
    close_lists(played.lists(), (&scaled(KEY, 0.375), &scaled(KEY, 0.5)));
    // And a pose it is flipped in is settled, its deform with its bones
    played.skeleton.settle();
    close_lists(played.lists(), (&scaled(KEY, 0.5), &scaled(KEY, 0.5)));
  }

  /// A leg of two bones whose IK constraint "leg" (setup: mix 1, softness
  /// 0, bend positive) reaches for "foot", and a "pelvis" whose transform
  /// constraint "level" (setup: every mix 0) copies the foot. "walk" keys
  /// the leg's mix 0.5, softness 1, bent the other way, and level's rotate
  /// and x mixes 0.5 and 0.25
  fn legged() -> SkeletonData {
    legged_as(Some(true))
  }

  /// [`legged`] with "leg" an IK constraint (`Some(true)`), a transform
  /// constraint on the thigh with every mix 0 (`Some(false)`), or not there
  /// (`None`); "walk" keys "leg" only when it is the IK one
  fn legged_as(leg: Option<bool>) -> SkeletonData {
    use crate::core::skeleton::{ConstraintId, Curve, IkSetup, IkValues, TransformMix};
    let long = |x: f32| BoneSetup {
      x,
      length: 10.0,
      ..BoneSetup::default()
    };
    let ik = IkValues {
      mix: 0.5,
      softness: 1.0,
      bend: IkBend::Negative,
      compress: false,
      stretch: false,
      max_stretch: None,
    };
    let lean = TransformMix {
      rotate: 0.5,
      x: 0.25,
      ..TransformMix::default()
    };
    let builder = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("thigh", Some("root"), long(0.0))
      .bone("shin", Some("thigh"), long(10.0))
      .bone("foot", Some("root"), BoneSetup::default())
      .bone("pelvis", Some("root"), BoneSetup::default());
    let builder = match leg {
      Some(true) => builder.ik("leg", &["thigh", "shin"], "foot", IkSetup::default()),
      Some(false) => builder.transform_constraint("leg", &["thigh"], "foot", Default::default()),
      None => builder,
    };
    let level = ConstraintId(u8::from(leg.is_some()));
    let mut timelines = vec![Timeline::TransformMix {
      constraint: level,
      frames: vec![(0.0, lean, Curve::Linear)],
    }];
    if leg == Some(true) {
      timelines.insert(
        0,
        Timeline::Ik {
          constraint: ConstraintId(0),
          frames: vec![(0.0, ik, Curve::Linear)],
        },
      );
    }
    builder
      .transform_constraint("level", &["pelvis"], "foot", Default::default())
      .animation(Animation {
        name: Arc::from("walk"),
        duration: 1.0,
        timelines,
      })
      .build()
      .expect("a valid skeleton")
  }

  fn leg_values(skeleton: &Skeleton) -> crate::core::skeleton::IkValues {
    match skeleton.pose.constraints[0] {
      ConstraintValues::Ik(values) => values,
      other => panic!("{other:?}"),
    }
  }

  fn level_mix(skeleton: &Skeleton) -> crate::core::skeleton::TransformMix {
    match skeleton.pose.constraints[1] {
      ConstraintValues::Transform(mix) => mix,
      other => panic!("{other:?}"),
    }
  }

  fn walking() -> Played {
    let mut played = Played::new(legged());
    played.play("walk", true);
    played
  }

  fn ik_override(o: IkOverride) -> ConstraintOverride {
    ConstraintOverride::Ik(o)
  }

  #[test]
  fn an_override_beats_the_animation_and_clearing_it_gives_the_animation_back() {
    let mut played = walking();
    // Set before the data lands: kept by name until it does
    played.skeleton.override_constraint(
      "leg",
      ik_override(IkOverride {
        mix: Some(0.25),
        ..IkOverride::default()
      }),
    );
    let along = TransformMixOverride {
      x: Some(1.0),
      ..TransformMixOverride::default()
    };
    played
      .skeleton
      .override_constraint("level", ConstraintOverride::Transform(along));
    played.step(0.1);
    // Each value given replaces the animated one; the rest are animated
    let leg = leg_values(&played.skeleton);
    close(leg.mix, 0.25);
    close(leg.softness, 1.0);
    assert_eq!(leg.bend, IkBend::Negative);
    let level = level_mix(&played.skeleton);
    close(level.rotate, 0.5);
    close(level.x, 1.0);
    // Set again, it replaces the override
    played.skeleton.override_constraint(
      "leg",
      ik_override(IkOverride {
        softness: Some(3.0),
        ..IkOverride::default()
      }),
    );
    played.step(0.1);
    let leg = leg_values(&played.skeleton);
    close(leg.mix, 0.5);
    close(leg.softness, 3.0);
    // Cleared, from the next step the animation has them again
    played.skeleton.clear_constraint_override("leg");
    played.skeleton.clear_constraint_override("level");
    played.step(0.1);
    let leg = leg_values(&played.skeleton);
    close(leg.mix, 0.5);
    close(leg.softness, 1.0);
    close(level_mix(&played.skeleton).x, 0.25);
  }

  #[test]
  fn an_override_forces_a_switch() {
    let mut played = walking();
    played.step(0.1);
    assert_eq!(leg_values(&played.skeleton).bend, IkBend::Negative);
    // The animation keys the leg bent the other way, not stretching: the
    // override bends it the positive way and stretches it
    played.skeleton.override_constraint(
      "leg",
      ik_override(IkOverride {
        bend: Some(IkBend::Positive),
        stretch: Some(true),
        ..IkOverride::default()
      }),
    );
    played.step(0.1);
    let leg = leg_values(&played.skeleton);
    assert!(leg.bend == IkBend::Positive && leg.stretch && !leg.compress);
    close(leg.mix, 0.5);
  }

  #[test]
  fn an_override_sets_the_stretch_limit_and_drops_one_below_one() {
    let log = capture_log();
    let mut played = walking();
    played.step(0.1);
    assert_eq!(leg_values(&played.skeleton).max_stretch, None);
    played.skeleton.override_constraint(
      "leg",
      ik_override(IkOverride {
        max_stretch: Some(1.5),
        ..IkOverride::default()
      }),
    );
    played.step(0.1);
    assert_eq!(leg_values(&played.skeleton).max_stretch, Some(1.5));
    // Below 1, or not finite: dropped with one warning, and the setup's
    // (none) stands
    for bad in [0.5, f32::NAN] {
      played.skeleton.override_constraint(
        "leg",
        ik_override(IkOverride {
          max_stretch: Some(bad),
          ..IkOverride::default()
        }),
      );
      played.step(0.1);
      assert_eq!(leg_values(&played.skeleton).max_stretch, None, "{bad}");
    }
    assert_eq!(log.containing("not finite"), 1, "{:?}", log.warnings());
    // Cleared, it is the setup's again
    played.skeleton.clear_constraint_override("leg");
    played.step(0.1);
    assert_eq!(leg_values(&played.skeleton).max_stretch, None);
    // A chain has no stretch to limit: the same override leaves a chain's
    // values untouched, and a bone count of 2 or less takes it
    let o = ConstraintOverride::Ik(IkOverride {
      max_stretch: Some(1.5),
      ..IkOverride::default()
    });
    for (bones, taken) in [(1, Some(1.5)), (2, Some(1.5)), (3, None), (16, None)] {
      let mut values = ConstraintValues::Ik(crate::core::skeleton::IkValues::of(
        &crate::core::skeleton::IkSetup::default(),
        bones,
      ));
      let before = values;
      o.apply(&mut values, bones);
      match values {
        ConstraintValues::Ik(after) => assert_eq!(after.max_stretch, taken, "{bones} bones"),
        other => panic!("{other:?}"),
      }
      if taken.is_none() {
        assert_eq!(values, before, "{bones} bones");
      }
    }
  }

  #[test]
  fn an_override_for_an_unknown_constraint_or_of_the_other_kind_warns_once() {
    let log = capture_log();
    let mut played = walking();
    // Before the data lands nothing can be checked: kept, and told once it
    // lands
    played
      .skeleton
      .override_constraint("ghost", ik_override(IkOverride::default()));
    played.step(0.1);
    played.step(0.1);
    assert_eq!(log.containing("\"ghost\""), 1, "{:?}", log.warnings());
    // Once it has landed, a name the data lacks warns once and does nothing
    let mixed = ik_override(IkOverride {
      mix: Some(0.25),
      ..IkOverride::default()
    });
    for _ in 0..3 {
      played.skeleton.override_constraint("nope", mixed);
      played.step(0.1);
    }
    assert_eq!(log.containing("\"nope\""), 1, "{:?}", log.warnings());
    // An override of the other kind warns once and changes nothing, not even
    // an override the constraint already has
    played.skeleton.override_constraint("leg", mixed);
    let wrong = ConstraintOverride::Transform(TransformMixOverride {
      rotate: Some(1.0),
      ..TransformMixOverride::default()
    });
    for _ in 0..3 {
      played.skeleton.override_constraint("leg", wrong);
      played.skeleton.override_constraint("level", mixed);
      played.step(0.1);
    }
    close(leg_values(&played.skeleton).mix, 0.25);
    close(level_mix(&played.skeleton).rotate, 0.5);
    assert_eq!(log.containing("\"leg\""), 1, "{:?}", log.warnings());
    assert_eq!(log.containing("\"level\""), 1, "{:?}", log.warnings());
    assert_eq!(log.warnings().len(), 4, "{:?}", log.warnings());
  }

  #[test]
  fn an_override_value_not_finite_is_dropped_with_one_warning() {
    let log = capture_log();
    let mut played = walking();
    played.step(0.1);
    played.skeleton.override_constraint(
      "leg",
      ik_override(IkOverride {
        mix: Some(f32::NAN),
        softness: Some(3.0),
        ..IkOverride::default()
      }),
    );
    played.skeleton.override_constraint(
      "level",
      ConstraintOverride::Transform(TransformMixOverride {
        rotate: Some(f32::INFINITY),
        x: Some(-2.0),
        ..TransformMixOverride::default()
      }),
    );
    played.step(0.1);
    // What is finite applies; the rest is animated
    let leg = leg_values(&played.skeleton);
    close(leg.mix, 0.5);
    close(leg.softness, 3.0);
    let level = level_mix(&played.skeleton);
    close(level.rotate, 0.5);
    close(level.x, -2.0);
    assert_eq!(log.containing("not finite"), 1, "{:?}", log.warnings());
    // An IK mix is taken within 0 to 1 and a softness below 0 as 0; a
    // transform mix outside 0 to 1 is kept
    played.skeleton.override_constraint(
      "leg",
      ik_override(IkOverride {
        mix: Some(2.0),
        softness: Some(-1.0),
        ..IkOverride::default()
      }),
    );
    played.step(0.1);
    let leg = leg_values(&played.skeleton);
    close(leg.mix, 1.0);
    close(leg.softness, 0.0);
    assert_eq!(log.warnings().len(), 1, "{:?}", log.warnings());
  }

  fn leg_mix(mix: f32) -> ConstraintOverride {
    ik_override(IkOverride {
      mix: Some(mix),
      ..IkOverride::default()
    })
  }

  #[test]
  fn an_override_kept_for_a_constraint_the_data_lacks_takes_a_later_call() {
    let log = capture_log();
    let mut played = walking();
    played.skeleton.override_constraint("leg", leg_mix(0.25));
    played.step(0.1);
    close(leg_values(&played.skeleton).mix, 0.25);
    // New data without "leg": the override is kept by name, and a call
    // while the data lacks the name warns no more and changes what is kept
    played.reload(legged_as(None));
    played.step(0.1);
    played.skeleton.override_constraint("leg", leg_mix(0.75));
    played.step(0.1);
    assert_eq!(log.containing("\"leg\""), 1, "{:?}", log.warnings());
    // Data with "leg" again: the later call's value holds it
    played.reload(legged());
    played.step(0.1);
    close(leg_values(&played.skeleton).mix, 0.75);
    assert_eq!(log.warnings().len(), 1, "{:?}", log.warnings());
  }

  #[test]
  fn an_override_through_a_reload_that_turns_its_constraint_s_kind_warns_once_and_is_kept() {
    let log = capture_log();
    let mut played = walking();
    played.skeleton.override_constraint("leg", leg_mix(0.25));
    played.step(0.1);
    // New data in which "leg" is a transform constraint: the IK override
    // warns once, does nothing to it, and stays kept
    played.reload(legged_as(Some(false)));
    for _ in 0..3 {
      played.step(0.1);
    }
    assert_eq!(log.containing("\"leg\""), 1, "{:?}", log.warnings());
    assert_eq!(log.containing("not of its override's kind"), 1);
    let ConstraintValues::Transform(mix) = played.skeleton.pose.constraints[0] else {
      panic!("{:?}", played.skeleton.pose.constraints);
    };
    assert_eq!(mix, crate::core::skeleton::TransformMix::default());
    // A later call of the IK kind, while kept, is kept in its place
    played.skeleton.override_constraint("leg", leg_mix(0.6));
    // Back to IK: the kept override holds it again, and nothing more is
    // said
    played.reload(legged());
    played.step(0.1);
    close(leg_values(&played.skeleton).mix, 0.6);
    assert_eq!(log.warnings().len(), 1, "{:?}", log.warnings());
  }
}
