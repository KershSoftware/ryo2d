//! A skeleton in the world: the [`Skeleton`] component, what a game sets on
//! it (skins, attachments and bone overrides, each kept by name), and its
//! part of the fixed step. The module notes give the rules.

use std::sync::Arc;

use glam::{Affine2, Mat2, Vec2};

use super::apply::shortest;
use super::attachment::Attachment;
use super::data::{BoneId, DataVersion, SkeletonData, SkeletonIndex, SlotId};
use super::pose::{BonePose, Pose};
use super::scope::SkeletonStore;
use super::state::{FiredEvent, FiredTrackEvent, SkeletonAnimation};
use super::validate::{MAX_BONES, MAX_SLOTS};
use crate::core::asset::Handle;
use crate::core::ecs::kernel::{DeferredWorld, HookContext};
use crate::core::ecs::{Component, Renderable, Transform};

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

/// What kind of thing a name the data lacks was given as
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Named {
  Bone,
  Slot,
  Skin,
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
struct SetOverride {
  bone: Arc<str>,
  id: Option<BoneId>,
  value: BoneOverride,
  animated: Option<BonePose>,
}

/// What a skeleton's step fired: the event keys its animations crossed, in
/// time order, and what happened to its track entries
#[derive(Debug, Default)]
pub(crate) struct Fired {
  pub(crate) events: Vec<FiredEvent>,
  pub(crate) track_events: Vec<FiredTrackEvent>,
}

/// Which bones of a new pose took their last world transform from the pose
/// before it (a skeleton has at most 255 bones)
#[derive(Debug, Clone, Copy, Default)]
struct Carried([u64; 4]);

impl Carried {
  fn set(&mut self, bone: usize) {
    if let Some(word) = self.0.get_mut(bone / 64) {
      *word |= 1 << (bone % 64);
    }
  }

  fn has(&self, bone: usize) -> bool {
    self
      .0
      .get(bone / 64)
      .is_some_and(|word| word & (1 << (bone % 64)) != 0)
  }
}

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
/// What a game sets on it (skins, attachments, bone overrides) is its own,
/// not the data's, is kept by name, and shows from the skeleton's next
/// step; what it answers (`bone_world`, `point`, `bounding_box`) is as its
/// last step left it.
///
/// It is drawn when its entity has a [`Renderable::skeleton`]: its regions
/// where its bones are, sorted, lit, shadowed and lifted as a sprite is,
/// and mirrored about the entity's origin by that renderable's `flip_x`
/// and `flip_y`, its bones (as `bone_world` answers) and sockets with it.
///
/// To give an entity another skeleton, insert the new `Skeleton` over the
/// old one. Assigning a whole new value through a mutable reference
/// (`*skeleton = Skeleton::new(..)`) does not count as adding one: no
/// scene is recorded for it, and it has no pose until its next step
#[derive(Component, Debug, Clone)]
#[require(Transform)]
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
  /// skeleton lacks, and until the data has loaded and a step has run
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
  /// changes
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

  /// The point attachment called `name` that one of the skeleton's slots
  /// shows (the first such slot in setup order), in the world: where it is
  /// and the way it points, in radians. `None` when no slot shows a point
  /// of that name: a point an animation or the game has hidden is not found
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
  /// can so turn a hit box on and off by keying its slot's attachment
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
  pub(crate) fn attachment(&self, slot: SlotId, name: &str) -> Option<&Attachment> {
    let index = self.index.as_ref()?;
    self
      .skins
      .iter()
      .rev()
      .filter_map(|worn| worn.skin)
      .chain(std::iter::once(0))
      .find_map(|skin| index.skins.get(skin)?.attachment(slot, name))
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
  /// transform becomes its current one
  fn settle(&mut self) {
    self.previous_space.clone_from(&self.pose.skeleton_space);
  }

  /// One fixed step. A step that sees other data than the last (the data
  /// landing, or a reload) first makes the pose anew and finds every kept
  /// name's place in it. Then: the animation state advances by `dt` and
  /// poses the skeleton (without one it holds its setup pose), the game's
  /// attachments and bone overrides are put over that, and the bones'
  /// world transforms are computed where it stands (`stand`: under the
  /// entity's transform, mirrored as its renderable flips it). The event
  /// keys crossed and the track events are pushed onto `fired`
  pub(crate) fn step(
    &mut self,
    data: &SkeletonData,
    seen: DataVersion,
    animation: Option<&mut SkeletonAnimation>,
    dt: f32,
    stand: Stand,
    fired: &mut Fired,
  ) {
    let mut carried = None;
    if self.seen == Some(seen) {
      // The transforms the last step computed become the previous ones,
      // and their buffer is computed over
      std::mem::swap(&mut self.previous_space, &mut self.pose.skeleton_space);
    } else {
      carried = Some(self.rebuild(data));
      self.seen = Some(seen);
    }
    match animation {
      Some(animation) => animation.advance_and_apply(
        data,
        &mut self.pose,
        dt,
        &mut fired.events,
        &mut fired.track_events,
      ),
      None => self.pose.reset(data),
    }
    self.finish(data, stand, carried);
  }

  /// A pose outside a step, for a skeleton that has none for `data` as it
  /// now is (just added, or its data just landed or reloaded): the pose is
  /// made anew and the animation state poses it where its tracks stand,
  /// an entry that has yet to start at its first moment. Nothing advances
  /// and nothing is fired: an entry's `Start` and the keys at its start
  /// go out with its first step, which advances it from 0 as if this had
  /// not run. With no state it is the setup pose. No step made it, so it
  /// is drawn as it is, not between it and the pose before
  pub(crate) fn appear(
    &mut self,
    data: &SkeletonData,
    seen: DataVersion,
    animation: Option<&mut SkeletonAnimation>,
    stand: Stand,
  ) {
    self.rebuild(data);
    self.seen = Some(seen);
    match animation {
      Some(animation) => animation.apply_unadvanced(data, &mut self.pose),
      None => self.pose.reset(data),
    }
    self.finish(data, stand, None);
    self.settle();
  }

  /// The end of a step and of a pose outside one: what the game set is put
  /// over the animation's result, and the bones are placed where it
  /// stands. After new data (`carried`), a bone with no transform from the
  /// pose before takes its current one as its previous one
  fn finish(&mut self, data: &SkeletonData, stand: Stand, carried: Option<Carried>) {
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
    for set in &mut self.overrides {
      set.animated = set
        .id
        .and_then(|bone| self.pose.bones.get(bone.index()).copied());
    }
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

  /// Computes the bones' world transforms again where it now stands, with
  /// the pose it has: the entity was moved after its skeleton's step (it
  /// sits on another skeleton's socket), or was flipped while no step
  /// runs. The bone overrides are applied afresh, as a world-space one
  /// depends on where it stands
  pub(crate) fn place_again(&mut self, data: &SkeletonData, stand: Stand) {
    for set in &self.overrides {
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
  /// its bone's parent, which is placed by then, overrides and all
  fn place(&mut self, data: &SkeletonData, stand: Stand) {
    self.root = stand.root;
    self.flip = stand.flip;
    let root = stand.affine();
    // Those of bones the data has come first: with none, the plain pass
    if self.overrides.first().is_none_or(|set| set.id.is_none()) {
      self.pose.compute_world(data, root);
      return;
    }
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
    self.pose.apply_root(root);
  }

  /// Makes the pose anew for `data` and finds each kept name's place in
  /// it. A bone the pose before had, by name, keeps its last transform in
  /// the skeleton's space as its previous one; the rest are marked not
  /// carried
  fn rebuild(&mut self, data: &SkeletonData) -> Carried {
    let index = data.index().clone();
    let pose = Pose::setup(data);
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
      Some((*world, self.attachment(slot, name)?))
    })
  }

  /// Whether one more of `count` attachments or overrides can be kept: a
  /// skeleton has at most `most` slots or bones to set them on, so past
  /// that the names are ones no data can have (set before the data landed,
  /// when they cannot be checked). One more is ignored, with one warning
  fn room(&mut self, count: usize, most: usize) -> bool {
    if count < most {
      return true;
    }
    if !self.warned_full {
      self.warned_full = true;
      log::warn!(
        "skeleton has {most} attachments or bone overrides set on names its data has yet to confirm; more are ignored"
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
    }
  }
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
fn apply_override(
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
fn invertible(linear: Mat2) -> bool {
  let determinant = linear.determinant();
  determinant.is_finite() && determinant.abs() > FLAT
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;

  use super::*;
  use crate::core::asset::Assets;
  use crate::core::ecs::World;
  use crate::core::skeleton::{BoneSetup, Inherit};
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
}
