//! 2D skeletal animation (being rebuilt; see the module notes).
//!
//! [`SkeletonData`] is the immutable asset: bones, slots, image regions,
//! skins of attachments, animations of timelines, and events, following
//! Spine's model. Build one in code with [`SkeletonData::builder`], or load
//! a `.skeleton.ron` file.
//!
//! A [`Skeleton`] component shows one on an entity, posed by the entity's
//! [`SkeletonAnimation`] in each fixed step; a [`BoneSocket`] on a child
//! entity rides one of its bones; and [`SkeletonEvent`] and
//! [`SkeletonTrackEvent`] messages tell the game what its animations did.
//! An entity with a `Renderable::skeleton()` draws its skeleton, sorted,
//! lit and shadowed as a sprite is.

mod animation;
mod apply;
mod attachment;
mod builder;
mod component;
mod curve;
mod data;
pub(crate) mod debug;
mod draw;
mod event;
mod pose;
mod scope;
mod skin;
mod socket;
mod state;
mod system;
mod validate;

pub use animation::{Animation, EventValues, Frame, Timeline};
pub use apply::MixBlend;
pub use attachment::{Attachment, BoundingBoxAttachment, PointAttachment, RegionAttachment};
pub use builder::{BoneSetup, SkeletonDataBuilder, SlotSetup};
pub use component::{BoneOverride, Skeleton, Space};
pub use curve::Curve;
pub use data::{
  BoneData, BoneId, EventAudio, EventData, Inherit, Region, SkeletonData, SlotBlend, SlotData,
  SlotId,
};
pub(crate) use draw::{Carrier, RegionQuad};
pub use event::{SkeletonEvent, SkeletonTrackEvent};
pub use pose::BonePose;
pub(crate) use scope::{SkeletonStore, hold_scenes, in_scene, release_scene};
pub use skin::Skin;
pub use socket::BoneSocket;
pub(crate) use socket::skip_sockets;
pub use state::{SkeletonAnimation, TrackEntry, TrackEntryId, TrackEventKind};
pub(crate) use system::{pose_new_skeletons, update_skeletons};
pub use validate::SkeletonError;
