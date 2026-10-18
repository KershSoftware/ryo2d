//! The state components a UI node spawns with, and the resource its walks keep

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::kernel::{Component, Entity, Resource};
use crate::core::ui::{Anchor, Rect, UiId};

use super::events::UiEvent;
use super::node::NodeState;

/// The root of a spawned tree
///
/// Its placement is the game's to change: edit `anchor`, `offset` or `at` on
/// the entity and the tree moves. A text, separator or spacer root ignores
/// placement; wrap a lone text in a `column()` to place it.
#[derive(Component, Debug, Clone)]
pub struct UiRoot {
  /// Which point of the screen the tree is placed against: the root's
  /// `.anchor(..)`, else the top-left (the centre for a modal root, as
  /// `ui.modal()`)
  pub anchor: Anchor,
  /// Shift from the anchor, in UI units
  pub offset: Vec2,
  /// An exact rectangle, instead of the anchor
  pub at: Option<Rect>,
  pub(crate) scene: u64,
  pub(crate) seq: u64,
  pub(crate) embedded: bool,
}

/// A node's name, as `.name(..)` gave it
#[derive(Component, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UiName(pub String);

/// The text of a text, button, checkbox or toggle node
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct UiLabel(pub String);

/// The sprite of an image node
#[derive(Component, Debug, Clone)]
pub struct UiImage(pub SpriteHandle);

/// Whether a checkbox or toggle is on
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UiChecked(pub bool);

/// A slider's value, or a progress bar's fraction
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct UiValue(pub f32);

/// The selected option of a radio group, dropdown, tabs or choice list
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UiIndex(pub usize);

/// The options of a radio group, dropdown, tabs or choice list
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct UiOptions(pub Vec<String>);

/// The text of a text input
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct UiTextValue(pub String);

/// What an inventory-style slot holds
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct UiSlot {
  /// The icon drawn in the slot
  pub icon: Option<SpriteHandle>,
  /// The stack count, drawn when above one
  pub count: u32,
  /// Whether the slot is drawn selected
  pub selected: bool,
}

/// A tooltip shown while the node is hovered or focused
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct UiTooltip(pub String);

/// Marks a node (and its subtree) as neither drawn nor laid out
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct UiHidden;

/// Marks a node as greyed out and not interactive
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct UiDisabled;

/// A node's pointer and focus state, as of the last draw
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiInteraction {
  pub(crate) id: UiId,
  pub(crate) hovered: bool,
  pub(crate) focused: bool,
}

impl UiInteraction {
  /// Whether the pointer is over the node
  pub fn hovered(&self) -> bool {
    self.hovered
  }

  /// Whether the node has focus
  pub fn focused(&self) -> bool {
    self.focused
  }

  /// The node's immediate-mode id, once it has been drawn
  pub fn id(&self) -> Option<UiId> {
    (self.id != UiId::NONE).then_some(self.id)
  }
}

/// Where a node sits in its tree: the key it is matched by in an update, and
/// which instance of a repeated template it came from
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub(crate) struct UiTreeNode {
  pub(crate) key: String,
  pub(crate) instance: u64,
}

/// The top of a `spawn_ui_in` subtree
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct UiAdded;

/// What a node's tree last gave it, so an update writes only what the tree
/// changed and leaves what the game or the player wrote
#[derive(Component, Debug, Clone, Default)]
pub(crate) struct UiApplied {
  pub(crate) state: NodeState,
  pub(crate) tooltip: Option<String>,
  pub(crate) hidden: bool,
  pub(crate) disabled: bool,
  /// A root's placement as the tree gave it
  pub(crate) place: Option<(Anchor, Vec2, Option<Rect>)>,
}

/// A confirm's No button: the node Back answers as if it were clicked
#[derive(Component)]
pub(crate) struct BackAnswer(pub(crate) Entity);

/// The trees the engine is walking this frame
#[derive(Resource, Default)]
pub(crate) struct UiNodes {
  /// (scene scope, root), in spawn order
  pub(crate) roots: Vec<(u64, Entity)>,
  pub(crate) seq: u64,
  pub(crate) pending_focus: Option<Entity>,
  /// Embedded trees' events, delivered after the scene's ui
  pub(crate) pending: Vec<UiEvent>,
  /// Embedded roots drawn this pass
  pub(crate) embedded_drawn: Vec<Entity>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_interaction_has_an_id_only_once_drawn() {
    let fresh = UiInteraction::default();
    assert_eq!(fresh.id(), None);
    assert!(!fresh.hovered() && !fresh.focused());
  }
}
