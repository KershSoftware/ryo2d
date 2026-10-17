//! Retained UI nodes: a tree described once, spawned as entities
//!
//! `node` is the tree value, `components` the state components and the
//! resource the walks keep, `events` what a widget reports, `spawn` the
//! entities a tree becomes and `draw` the walk that draws them each frame.

mod components;
mod draw;
mod events;
mod node;
mod spawn;

pub use components::{
  UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiInteraction, UiLabel, UiName, UiOptions,
  UiRoot, UiSlot, UiTextValue, UiTooltip, UiValue,
};
pub use events::{UiEvent, UiEventKind};
pub use node::{TextRole, UiNode, UiNodeKind};

pub(crate) use components::UiNodes;
pub(crate) use draw::{begin_pass, draw_nodes};
pub(crate) use events::deliver;
pub(crate) use spawn::*;
