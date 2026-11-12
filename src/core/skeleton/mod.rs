//! 2D skeletal animation (being rebuilt; see the module notes).
//!
//! [`SkeletonData`] is the immutable asset: bones, slots, image regions,
//! skins of attachments, animations of timelines, and events, following
//! Spine's model. Build one in code with [`SkeletonData::builder`].

mod animation;
mod apply;
mod attachment;
mod builder;
mod curve;
mod data;
mod pose;
mod skin;
mod state;
mod validate;

pub use animation::{Animation, EventValues, Frame, Timeline};
pub use apply::MixBlend;
pub use attachment::{Attachment, BoundingBoxAttachment, PointAttachment, RegionAttachment};
pub use builder::{BoneSetup, SkeletonDataBuilder, SlotSetup};
pub use curve::Curve;
pub use data::{
  BoneData, BoneId, EventAudio, EventData, Inherit, Region, SkeletonData, SlotBlend, SlotData,
  SlotId,
};
pub use pose::BonePose;
pub use skin::Skin;
pub use state::{SkeletonAnimation, TrackEntry, TrackEntryId, TrackEventKind};
pub use validate::SkeletonError;
