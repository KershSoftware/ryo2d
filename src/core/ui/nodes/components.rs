//! The state components a UI node spawns with, and the resource its walks keep

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::kernel::{Component, Entity, Resource, World};
use crate::core::text::TextReveal;
use crate::core::ui::{Anchor, Rect, UiId};

use super::events::UiEvent;
use super::node::UiNodeKind;

/// The root of a spawned tree
///
/// Its placement is the game's to change: edit `anchor`, `offset` or `at` on
/// the entity and the tree moves.
#[derive(Component, Debug, Clone)]
pub struct UiRoot {
  /// Which point of the screen the tree is placed against
  pub anchor: Anchor,
  /// Shift from the anchor, in UI units
  pub offset: Vec2,
  /// An exact rectangle, instead of the anchor
  pub at: Option<Rect>,
  #[allow(dead_code)] // recorded at spawn; nothing reads it yet
  pub(crate) scene: u64,
  #[allow(dead_code)] // recorded at spawn; nothing reads it yet
  pub(crate) seq: u64,
  pub(crate) embedded: bool,
}

/// A node's name, as `.name(..)` gave it
#[derive(Component, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UiName(pub String);

/// The text of a text, button, checkbox, toggle, slider, progress or rebind node
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
#[derive(Component, Debug, Clone, Default)]
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

/// A confirm's No button: the node Back answers as if it were clicked
#[derive(Component)]
#[allow(dead_code)] // read once node dialogs answer Back
pub(crate) struct BackAnswer(pub(crate) Entity);

/// The trees the engine is walking this frame
#[derive(Resource, Default)]
pub(crate) struct UiNodes {
  /// (scene scope, root), in spawn order
  pub(crate) roots: Vec<(u64, Entity)>,
  pub(crate) seq: u64,
  pub(crate) pending_focus: Option<Entity>,
  /// Embedded trees' events, delivered after the scene's ui
  #[allow(dead_code)] // read once trees can be embedded
  pub(crate) pending: Vec<UiEvent>,
  /// Embedded roots drawn this pass
  pub(crate) embedded_drawn: Vec<Entity>,
}

/// Register every component a node may carry. The walk reads a node's
/// optional components in one tuple, which fails for a component type the
/// world has never registered.
pub(crate) fn register_node_components(world: &mut World) {
  world.register_component::<UiNodeKind>();
  world.register_component::<UiRoot>();
  world.register_component::<UiName>();
  world.register_component::<UiLabel>();
  world.register_component::<UiImage>();
  world.register_component::<UiChecked>();
  world.register_component::<UiValue>();
  world.register_component::<UiIndex>();
  world.register_component::<UiOptions>();
  world.register_component::<UiTextValue>();
  world.register_component::<UiSlot>();
  world.register_component::<UiTooltip>();
  world.register_component::<UiHidden>();
  world.register_component::<UiDisabled>();
  world.register_component::<UiInteraction>();
  world.register_component::<BackAnswer>();
  world.register_component::<TextReveal>();
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
