//! The skeleton asset: bones, slots, regions, skins, animations and events,
//! immutable once built and checked.

use std::collections::HashMap;
use std::sync::Arc;

use super::animation::Animation;
use super::builder::SkeletonDataBuilder;
use super::skin::Skin;
use crate::core::asset::{Asset, Handle};
use crate::core::graphics::Image;
use crate::core::ui::Color;

/// A bone's index in [`SkeletonData::bones`]. A skeleton has at most 255
/// bones, so an index fits a `u8`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BoneId(pub u8);

impl BoneId {
  /// The index as a `usize`, for indexing the bone list
  pub fn index(self) -> usize {
    usize::from(self.0)
  }
}

/// A slot's index in [`SkeletonData::slots`] (its place in the setup draw
/// order). A skeleton has at most 255 slots, so an index fits a `u8`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotId(pub u8);

impl SlotId {
  /// The index as a `usize`, for indexing the slot list
  pub fn index(self) -> usize {
    usize::from(self.0)
  }
}

/// Which of its parent's transform a bone takes on (Spine's five modes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Inherit {
  /// The parent's full transform: translation, rotation, scale, shear and
  /// reflection
  #[default]
  Normal,
  /// Only the parent's translation
  OnlyTranslation,
  /// Everything but the parent's rotation and reflection
  NoRotationOrReflection,
  /// Everything but the parent's scale
  NoScale,
  /// Everything but the parent's scale and reflection
  NoScaleOrReflection,
}

/// One bone of a skeleton and its setup pose. Angles are radians, y down and
/// positive rotation clockwise, as in the rest of the engine
#[derive(Debug, Clone, PartialEq)]
pub struct BoneData {
  pub name: Arc<str>,
  /// The parent bone, always earlier in the list; `None` for a root
  pub parent: Option<BoneId>,
  /// The bone's length, for debug drawing and IK
  pub length: f32,
  pub x: f32,
  pub y: f32,
  /// Radians
  pub rotation: f32,
  pub scale_x: f32,
  pub scale_y: f32,
  /// Radians
  pub shear_x: f32,
  /// Radians
  pub shear_y: f32,
  pub inherit: Inherit,
}

/// How a slot's attachment blends with what is drawn under it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SlotBlend {
  #[default]
  Normal,
  Additive,
  Multiply,
  Screen,
}

/// One slot of a skeleton: a place on a bone where one attachment at a time
/// shows, with its setup colour and attachment. Slots are listed in setup
/// draw order
#[derive(Debug, Clone, PartialEq)]
pub struct SlotData {
  pub name: Arc<str>,
  pub bone: BoneId,
  /// The setup colour (sRGB with alpha), multiplied into the attachment's
  pub color: Color,
  /// The setup attachment's name, looked up in the skin, or none
  pub attachment: Option<Arc<str>>,
  pub blend: SlotBlend,
}

/// A named pixel rectangle of an image: one part of an atlas, or a whole
/// image for one part
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
  pub name: Arc<str>,
  pub image: Handle<Image>,
  pub x: u32,
  pub y: u32,
  pub width: u32,
  pub height: u32,
}

/// An event a skeleton's animations can fire, with its default values. An
/// event key can override them
#[derive(Debug, Clone, PartialEq)]
pub struct EventData {
  pub name: Arc<str>,
  pub int: i32,
  pub float: f32,
  pub string: Arc<str>,
  /// A sound for the game to play when the event fires; the engine does not
  /// play it itself
  pub audio: Option<EventAudio>,
}

impl EventData {
  /// An event with zero values, an empty string and no audio
  pub fn new(name: impl Into<Arc<str>>) -> Self {
    Self {
      name: name.into(),
      int: 0,
      float: 0.0,
      string: Arc::from(""),
      audio: None,
    }
  }
}

/// The sound an event names: a path (a string, not a loaded asset), with its
/// volume and stereo balance
#[derive(Debug, Clone, PartialEq)]
pub struct EventAudio {
  pub path: Arc<str>,
  pub volume: f32,
  /// -1 (left) to 1 (right)
  pub balance: f32,
}

/// A skeleton: its bones, slots, image regions, skins, animations and
/// events. Built in code with [`SkeletonData::builder`] (or read from a
/// file), checked once, and shared by every skeleton entity that uses it
#[derive(Debug, Clone)]
pub struct SkeletonData {
  pub(super) bones: Vec<BoneData>,
  pub(super) slots: Vec<SlotData>,
  pub(super) regions: Vec<Region>,
  pub(super) animations: Vec<Animation>,
  pub(super) events: Vec<EventData>,
  /// The names and the skins, shared with every skeleton entity that uses
  /// this data
  pub(super) index: Arc<SkeletonIndex>,
  /// The animations' names, durations and mixes, shared with every
  /// animation state that plays this data
  pub(super) catalogue: Arc<AnimationCatalogue>,
}

/// What a skeleton entity needs of its data when a game asks it for a bone,
/// a point or a bounding box by name, or names a skin: the names, each
/// slot's bone, and the skins. Shared, so a skeleton keeps it without
/// borrowing the data, and a reload (new data) is a new index
#[derive(Debug)]
pub(crate) struct SkeletonIndex {
  pub(super) bone_names: HashMap<Arc<str>, BoneId>,
  pub(super) slot_names: HashMap<Arc<str>, SlotId>,
  pub(super) skin_names: HashMap<Arc<str>, usize>,
  /// The default skin is always first
  pub(super) skins: Vec<Skin>,
  /// Each slot's bone, by slot index
  pub(super) slot_bones: Vec<BoneId>,
}

/// What an animation state needs to know of a skeleton's animations when a
/// call names one: each name's index and duration, and the mixes. Shared, so
/// a state keeps it without borrowing the data, and a reload (new data) is a
/// new catalogue
#[derive(Debug)]
pub(crate) struct AnimationCatalogue {
  pub(super) names: HashMap<Arc<str>, usize>,
  pub(super) durations: Vec<f32>,
  /// Mix durations by (from, to) animation index
  pub(super) mixes: HashMap<(usize, usize), f32>,
  pub(super) default_mix: f32,
}

impl AnimationCatalogue {
  /// The animation called `name`, as an index
  pub(crate) fn index(&self, name: &str) -> Option<usize> {
    self.names.get(name).copied()
  }

  /// The animation called `name`: its index, and the data's own copy of
  /// the name, to share
  pub(crate) fn find(&self, name: &str) -> Option<(usize, &Arc<str>)> {
    self
      .names
      .get_key_value(name)
      .map(|(name, &index)| (index, name))
  }

  /// The animation's duration (0 for an index out of range)
  pub(crate) fn duration(&self, index: usize) -> f32 {
    self.durations.get(index).copied().unwrap_or(0.0)
  }

  /// The mix from one animation to another by index: the pair's own, else
  /// the default. `None` (no animation, or the empty animation) takes the
  /// default
  pub(crate) fn mix(&self, from: Option<usize>, to: Option<usize>) -> f32 {
    from
      .zip(to)
      .and_then(|pair| self.mixes.get(&pair).copied())
      .unwrap_or(self.default_mix)
  }
}

impl Asset for SkeletonData {}

impl SkeletonData {
  /// The name of the skin every skeleton has, which attachments go into
  /// unless they name another
  pub const DEFAULT_SKIN: &'static str = "default";

  /// Starts building a skeleton in code
  pub fn builder() -> SkeletonDataBuilder {
    SkeletonDataBuilder::new()
  }

  /// The bones, parents before children
  pub fn bones(&self) -> &[BoneData] {
    &self.bones
  }

  /// The slots, in setup draw order
  pub fn slots(&self) -> &[SlotData] {
    &self.slots
  }

  /// The image regions, which region attachments index
  pub fn regions(&self) -> &[Region] {
    &self.regions
  }

  /// Every skin, the default skin first
  pub fn skins(&self) -> &[Skin] {
    &self.index.skins
  }

  /// The skin named [`SkeletonData::DEFAULT_SKIN`], which every skeleton has
  /// (it may be empty)
  pub fn default_skin(&self) -> &Skin {
    &self.index.skins[0]
  }

  /// The skin called `name`
  pub fn skin(&self, name: &str) -> Option<&Skin> {
    let index = &self.index;
    index.skin_names.get(name).map(|&skin| &index.skins[skin])
  }

  pub fn animations(&self) -> &[Animation] {
    &self.animations
  }

  /// The events the animations fire, which event keys index
  pub fn events(&self) -> &[EventData] {
    &self.events
  }

  /// The bone called `name`
  pub fn bone(&self, name: &str) -> Option<BoneId> {
    self.index.bone_names.get(name).copied()
  }

  /// The slot called `name`
  pub fn slot(&self, name: &str) -> Option<SlotId> {
    self.index.slot_names.get(name).copied()
  }

  /// The animation called `name`
  pub fn animation(&self, name: &str) -> Option<&Animation> {
    self
      .catalogue
      .index(name)
      .and_then(|index| self.animations.get(index))
  }

  /// The mix duration, in seconds, from animation `from` to `to`: the pair's
  /// own, else the default mix
  pub fn mix(&self, from: &str, to: &str) -> f32 {
    let catalogue = &self.catalogue;
    match (catalogue.index(from), catalogue.index(to)) {
      (Some(from), Some(to)) => catalogue.mix(Some(from), Some(to)),
      _ => catalogue.default_mix,
    }
  }

  /// The mix used between animations with no mix of their own
  pub fn default_mix(&self) -> f32 {
    self.catalogue.default_mix
  }

  /// The animations' names, durations and mixes, shared
  pub(crate) fn catalogue(&self) -> &Arc<AnimationCatalogue> {
    &self.catalogue
  }

  /// The names and the skins, shared
  pub(crate) fn index(&self) -> &Arc<SkeletonIndex> {
    &self.index
  }
}
