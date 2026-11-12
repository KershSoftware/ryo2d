//! Building a skeleton in code (the loader builds through it too).

use std::sync::Arc;

use super::animation::Animation;
use super::attachment::Attachment;
use super::data::{BoneId, EventData, Inherit, Region, SkeletonData, SlotBlend, SlotId};
use super::validate::{self, SkeletonError};
use crate::core::ui::Color;

/// A bone's setup pose, for [`SkeletonDataBuilder::bone`]. The default is
/// at the parent's origin, unrotated, at unit scale, with no length
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneSetup {
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

impl Default for BoneSetup {
  fn default() -> Self {
    Self {
      length: 0.0,
      x: 0.0,
      y: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      shear_x: 0.0,
      shear_y: 0.0,
      inherit: Inherit::Normal,
    }
  }
}

/// A slot's setup, for [`SkeletonDataBuilder::slot`]. The default is
/// white, with no attachment and normal blending
#[derive(Debug, Clone, PartialEq)]
pub struct SlotSetup {
  pub color: Color,
  pub attachment: Option<Arc<str>>,
  pub blend: SlotBlend,
}

impl Default for SlotSetup {
  fn default() -> Self {
    Self {
      color: Color::WHITE,
      attachment: None,
      blend: SlotBlend::Normal,
    }
  }
}

#[derive(Debug, Clone)]
pub(super) struct PendingBone {
  pub name: Arc<str>,
  pub parent: Option<Arc<str>>,
  pub setup: BoneSetup,
}

#[derive(Debug, Clone)]
pub(super) struct PendingSlot {
  pub name: Arc<str>,
  pub bone: Arc<str>,
  pub setup: SlotSetup,
}

#[derive(Debug, Clone)]
pub(super) struct PendingAttachment {
  pub skin: Arc<str>,
  pub slot: Arc<str>,
  pub name: Arc<str>,
  pub attachment: Attachment,
}

/// Builds a [`SkeletonData`] in code: bones (parents first), slots (in draw
/// order), image regions, skins and their attachments, animations, events
/// and mixes. Bones, slots and skins are referred to by name; animations
/// refer to bones and slots by [`BoneId`]/[`SlotId`] and to events by index,
/// which are their places in the order added (`bone_id`, `slot_id`,
/// `region_index` and `event_index` look them up). Nothing is checked until
/// [`build`](Self::build), which names the first bad item
#[derive(Debug, Clone, Default)]
pub struct SkeletonDataBuilder {
  pub(super) bones: Vec<PendingBone>,
  pub(super) slots: Vec<PendingSlot>,
  pub(super) regions: Vec<Region>,
  pub(super) skins: Vec<Arc<str>>,
  pub(super) attachments: Vec<PendingAttachment>,
  pub(super) animations: Vec<Animation>,
  pub(super) events: Vec<EventData>,
  pub(super) mixes: Vec<(Arc<str>, Arc<str>, f32)>,
  pub(super) default_mix: f32,
}

impl SkeletonDataBuilder {
  /// An empty skeleton with a default mix of 0
  pub fn new() -> Self {
    Self::default()
  }

  /// Adds a bone under `parent` (a bone added before it), or a root
  pub fn bone(mut self, name: impl Into<Arc<str>>, parent: Option<&str>, setup: BoneSetup) -> Self {
    self.bones.push(PendingBone {
      name: name.into(),
      parent: parent.map(Arc::from),
      setup,
    });
    self
  }

  /// Adds a slot on `bone`, after (drawn over) the slots already added
  pub fn slot(mut self, name: impl Into<Arc<str>>, bone: &str, setup: SlotSetup) -> Self {
    self.slots.push(PendingSlot {
      name: name.into(),
      bone: Arc::from(bone),
      setup,
    });
    self
  }

  /// Adds an image region; region attachments index regions in the order
  /// added
  pub fn region(mut self, region: Region) -> Self {
    self.regions.push(region);
    self
  }

  /// Declares a skin. The default skin
  /// ([`SkeletonData::DEFAULT_SKIN`]) always exists and need not be declared
  pub fn skin(mut self, name: impl Into<Arc<str>>) -> Self {
    self.skins.push(name.into());
    self
  }

  /// Puts `attachment`, called `name`, in `slot` of `skin`
  pub fn attachment(
    mut self,
    skin: &str,
    slot: &str,
    name: impl Into<Arc<str>>,
    attachment: Attachment,
  ) -> Self {
    self.attachments.push(PendingAttachment {
      skin: Arc::from(skin),
      slot: Arc::from(slot),
      name: name.into(),
      attachment,
    });
    self
  }

  pub fn animation(mut self, animation: Animation) -> Self {
    self.animations.push(animation);
    self
  }

  /// Adds an event; event keys index events in the order added
  pub fn event(mut self, event: EventData) -> Self {
    self.events.push(event);
    self
  }

  /// Sets the mix duration, in seconds, from animation `from` to `to`
  pub fn mix(mut self, from: &str, to: &str, duration: f32) -> Self {
    self.mixes.push((Arc::from(from), Arc::from(to), duration));
    self
  }

  /// Sets the mix, in seconds, between animations with no mix of their own
  pub fn default_mix(mut self, duration: f32) -> Self {
    self.default_mix = duration;
    self
  }

  /// The id the first bone called `name` will have
  pub fn bone_id(&self, name: &str) -> Option<BoneId> {
    let index = self.bones.iter().position(|bone| &*bone.name == name)?;
    u8::try_from(index).ok().map(BoneId)
  }

  /// The id the first slot called `name` will have
  pub fn slot_id(&self, name: &str) -> Option<SlotId> {
    let index = self.slots.iter().position(|slot| &*slot.name == name)?;
    u8::try_from(index).ok().map(SlotId)
  }

  /// The index of the first region called `name`
  pub fn region_index(&self, name: &str) -> Option<usize> {
    self.regions.iter().position(|region| &*region.name == name)
  }

  /// The index of the first event called `name`
  pub fn event_index(&self, name: &str) -> Option<usize> {
    self.events.iter().position(|event| &*event.name == name)
  }

  /// Checks everything and builds the skeleton, or names the first item
  /// that is wrong and why
  pub fn build(self) -> Result<SkeletonData, SkeletonError> {
    validate::build(self)
  }
}
