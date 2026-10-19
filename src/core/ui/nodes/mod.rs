//! Retained UI nodes: a tree described once, spawned as entities
//!
//! `node` is the tree value, `components` the state components and the
//! resource the walks keep, `events` what a widget reports, `spawn` the
//! entities a tree becomes, `reconcile` the update of a spawned tree in place,
//! `draw` the walk that draws them each frame, `cache` what the walk keeps of
//! each node's last build and which nodes changed since, and `list` the
//! pooled list.

pub(crate) mod bindings;
mod cache;
mod components;
mod draw;
mod events;
mod list;
mod node;
#[cfg(all(test, feature = "default-fonts"))]
mod oracle;
#[cfg(all(test, feature = "default-fonts"))]
mod oracle_tests;
mod reconcile;
mod spawn;

pub use bindings::BindValue;
pub use components::{
  UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiInteraction, UiLabel, UiName, UiOptions,
  UiRoot, UiSlot, UiTextValue, UiTooltip, UiValue,
};
pub use events::{UiEvent, UiEventKind, UiListBind, UiReloaded};
pub use list::UiListRow;
pub use node::{TextRole, UiNode, UiNodeKind};

pub(crate) use cache::cache_stats;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use cache::{cache_records, node_snapshot, set_spot_check};
pub(crate) use cache::{check_cache_ticks, end_frame, set_cache_enabled};
pub(crate) use components::UiNodes;
pub(crate) use draw::{begin_pass, deliver_pending, draw_nodes};
pub(crate) use events::deliver;
pub(crate) use list::{RowShape, list_count, list_index, list_jump, list_refresh, update_lists};
#[cfg(test)]
pub(crate) use node::fingerprint;
pub(crate) use reconcile::{SourceQuery, reload_trees, update_ui};
pub(crate) use spawn::*;
