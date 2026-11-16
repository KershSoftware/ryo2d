//! 2D skeletal animation: characters built of parts on a hierarchy of
//! bones, posed by animations that are mixed, layered and queued.
//!
//! [`SkeletonData`] is the immutable asset: bones, slots, image regions,
//! skins of attachments, animations of timelines, and events, following
//! Spine's model. Build one in code with [`SkeletonData::builder`], or load
//! a `.skeleton.ron` file.
//!
//! A [`Skeleton`] component shows one on an entity, posed by the entity's
//! [`SkeletonAnimation`] in each fixed step, its constraints then bringing
//! bones to their targets or making them copy a target bone's transform
//! (springs, which give a bone secondary motion, are held in the data);
//! a [`BoneSocket`] on a child
//! entity rides one of its bones; and [`SkeletonEvent`] and
//! [`SkeletonTrackEvent`] messages tell the game what its animations did.
//! An entity with a `Renderable::skeleton()` draws its skeleton, its
//! regions and its meshes skinned to their bones and shaped by its
//! animations' deform keys, sorted, lit and shadowed as a sprite is, and
//! between fixed steps as an interpolated entity is.
//!
//! Not built: path constraints, clipping, linked meshes, and reading
//! another tool's files.

mod animation;
mod apply;
mod attachment;
mod builder;
mod component;
mod constrain;
mod constraint;
mod curve;
mod data;
pub(crate) mod debug;
mod draw;
mod event;
mod ik;
mod pose;
mod scope;
mod skin;
mod socket;
mod state;
mod system;
mod transform_constraint;
mod validate;

pub use animation::{Animation, EventValues, Frame, Timeline};
pub use apply::MixBlend;
pub use attachment::{
  Attachment, BoundingBoxAttachment, Influence, MeshAttachment, MeshVertices, PointAttachment,
  RegionAttachment, WeightedVertices,
};
pub use builder::{BoneSetup, SkeletonDataBuilder, SlotSetup};
pub use component::{
  BoneOverride, ConstraintOverride, IkOverride, Skeleton, Space, SpringOverride,
  TransformMixOverride,
};
pub use constraint::{
  Constraint, ConstraintId, IkBend, IkConstraint, IkSetup, MAX_CONSTRAINTS, MAX_IK_BONES,
  SpringConstraint, SpringSetup, SpringValues, TransformConstraint, TransformMix, TransformOffset,
  TransformSetup,
};
pub use curve::Curve;
pub use data::{
  BoneData, BoneId, EventAudio, EventData, Inherit, Region, SkeletonData, SlotBlend, SlotData,
  SlotId,
};
pub(crate) use draw::{Carrier, MeshPart, Part, Parts, RegionQuad};
pub use event::{SkeletonEvent, SkeletonTrackEvent};
pub use ik::IkValues;
pub(crate) use scope::{SkeletonStore, hold_scenes, in_scene, release_scene};
pub use skin::Skin;
pub use socket::BoneSocket;
pub(crate) use socket::skip_sockets;
pub use state::{SkeletonAnimation, TrackEntry, TrackEntryId, TrackEventKind};
pub(crate) use system::{pose_new_skeletons, update_skeletons};
pub use validate::SkeletonError;
