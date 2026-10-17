//! Retained UI nodes: a tree described once, spawned as entities
//!
//! `node` is the tree value, `components` the state components and the
//! resource the walks keep, `events` what a widget reports.

// The spawn, draw and event tasks that read these fields and helpers land after
// the tree value; until then they are unused
#![allow(dead_code)]

mod components;
mod events;
mod node;
mod spawn;

pub use components::{
  UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiInteraction, UiLabel, UiName, UiOptions,
  UiRoot, UiSlot, UiTextValue, UiTooltip, UiValue,
};
pub use events::{UiEvent, UiEventKind};
pub use node::{TextRole, UiNode, UiNodeKind};

#[allow(unused_imports)]
pub(crate) use components::{BackAnswer, UiNodes};
#[allow(unused_imports)]
pub(crate) use node::{
  ButtonOpts, ImageOpts, Insert, Kind, NodeFlags, NodeLayout, NodeState, RebindOpts, ScrollOpts,
  SliderOpts, TextInputOpts, TextOpts,
};

pub(crate) use spawn::*;
