//! Bringing a spawned node in line with a new tree of the same kind

use crate::core::ecs::kernel::{Component, Entity, EntityWorldMut, World};
use crate::core::text::TextReveal;
use crate::core::ui::Anchor;

use super::bindings::{self, Pattern, UiBind, UiPattern};
use super::components::{
  UiApplied, UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiLabel, UiName, UiOptions, UiRoot,
  UiTextValue, UiTooltip, UiTreeNode, UiValue,
};
use super::node::{Kind, NodeState, UiNode, UiNodeKind};
use super::spawn::root_of;

/// Whether two kinds come from the same constructor
pub(crate) fn same_kind(a: &Kind, b: &Kind) -> bool {
  std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// What an update leaves for the caller: the node's children and the names
/// it refers to, which are resolved once the whole tree is in line
pub(crate) struct NodeUpdate {
  pub(crate) children: Vec<UiNode>,
  pub(crate) nav: [Option<String>; 4],
  pub(crate) popup: Option<String>,
  pub(crate) modal: bool,
  /// Its `UiBind` was added, removed or renamed
  pub(crate) rebound: bool,
}

/// Bring `entity` in line with `node`, which has the same kind
///
/// The tree's own settings are rewritten when they differ. State is written
/// only where the tree changed it since it last gave it (`UiApplied`), so what
/// the game or the player wrote survives, and a bound value is never written.
pub(crate) fn update_node(
  world: &mut World,
  entity: Entity,
  node: UiNode,
  key: String,
) -> NodeUpdate {
  let UiNode {
    kind,
    layout,
    flags,
    variant,
    action,
    name,
    tooltip,
    hidden,
    disabled,
    embedded,
    nav,
    state,
    children,
    inserts: _,
    given: _,
    bind,
    bound,
    seeds: _,
    key: _,
    template: _,
    instance,
  } = node;

  let popup = match &kind {
    Kind::Popup { anchor } => Some(anchor.clone()),
    _ => None,
  };
  let modal = flags.modal;
  let pattern = bound
    .then(|| UiPattern {
      label: state.label.as_deref().map(Pattern::parse),
      tooltip: tooltip.as_deref().map(Pattern::parse),
    })
    .filter(|p| p.label.is_some() || p.tooltip.is_some());
  let bind = bind.filter(|_| kind.takes("bind")).map(UiBind);
  let mut update = NodeUpdate {
    children,
    nav,
    popup,
    modal,
    rebound: false,
  };
  let Ok(mut node) = world.get_entity_mut(entity) else {
    return update;
  };

  let Some(applied) = node.get::<UiApplied>().cloned() else {
    return update;
  };
  let mut settings_changed = update_settings(
    &mut node,
    UiNodeKind {
      kind,
      layout,
      flags,
      variant,
      action,
      nav: [None; 4],
      anchor_node: None,
    },
    name.map(UiName),
    UiTreeNode { key, instance },
  );
  update.rebound = set(&mut node, bind);
  let pattern_changed = set(&mut node, pattern);
  settings_changed |= update.rebound || pattern_changed;

  let has_bind = node.contains::<UiBind>();
  let (label_pattern, tooltip_pattern) = node
    .get::<UiPattern>()
    .map_or((false, false), |p| (p.label.is_some(), p.tooltip.is_some()));
  update_state(&mut node, &applied, &state, has_bind, label_pattern);
  if !tooltip_pattern && tooltip != applied.tooltip {
    write(&mut node, tooltip.clone().map(UiTooltip));
  }
  if hidden != applied.hidden {
    write(&mut node, hidden.then_some(UiHidden));
  }
  if disabled != applied.disabled {
    write(&mut node, disabled.then_some(UiDisabled));
  }
  let place = update_place(&mut node, &applied, modal, embedded);

  if let Some(mut stored) = node.get_mut::<UiApplied>() {
    *stored = UiApplied {
      state,
      tooltip,
      hidden,
      disabled,
      place,
    };
  }

  if settings_changed {
    bindings::invalidate(world, entity);
  }
  if pattern_changed && let Some(root) = root_of(world, entity) {
    bindings::format_node(world, root, entity);
  }
  update
}

/// Insert `value`, or remove the component for `None`
fn write<C: Component>(node: &mut EntityWorldMut<'_>, value: Option<C>) {
  match value {
    Some(value) => {
      node.insert(value);
    }
    None => {
      node.remove::<C>();
    }
  }
}

/// Make the component `value`, writing only on a difference; whether it did
fn set<C: Component + PartialEq>(node: &mut EntityWorldMut<'_>, value: Option<C>) -> bool {
  if node.get::<C>() == value.as_ref() {
    return false;
  }
  write(node, value);
  true
}

/// Rewrite what the tree owns; whether anything differed
fn update_settings(
  node: &mut EntityWorldMut<'_>,
  new: UiNodeKind,
  name: Option<UiName>,
  tree: UiTreeNode,
) -> bool {
  let same = node.get::<UiNodeKind>().is_some_and(|old| {
    old.kind == new.kind
      && old.layout == new.layout
      && old.flags == new.flags
      && old.variant == new.variant
      && old.action == new.action
  });
  if !same && let Some(mut old) = node.get_mut::<UiNodeKind>() {
    old.kind = new.kind;
    old.layout = new.layout;
    old.flags = new.flags;
    old.variant = new.variant;
    old.action = new.action;
  }
  let named = set(node, name);
  let placed = set(node, Some(tree));
  !same || named || placed
}

/// The three-way rule for each state component: write only where the tree
/// changed what it gave
fn update_state(
  node: &mut EntityWorldMut<'_>,
  applied: &UiApplied,
  state: &NodeState,
  bound: bool,
  label_pattern: bool,
) {
  let old = &applied.state;
  if !label_pattern && state.label != old.label {
    write(node, state.label.clone().map(UiLabel));
  }
  if state.image != old.image {
    write(node, state.image.clone().map(UiImage));
  }
  if !bound {
    if state.checked != old.checked {
      write(node, state.checked.map(UiChecked));
    }
    if state.value != old.value {
      write(node, state.value.map(UiValue));
    }
    if state.index != old.index {
      write(node, state.index.map(UiIndex));
    }
    if state.text != old.text {
      write(node, state.text.clone().map(UiTextValue));
    }
  }
  if state.options != old.options {
    write(node, state.options.clone().map(UiOptions));
  }
  if state.slot != old.slot {
    write(node, state.slot.clone());
  }
  if !same_reveal(&state.reveal, &old.reveal) {
    write(node, state.reveal);
  }
}

/// Whether two reveals are set the same way, ignoring how far they have run
fn same_reveal(a: &Option<TextReveal>, b: &Option<TextReveal>) -> bool {
  match (a, b) {
    (None, None) => true,
    (Some(a), Some(b)) => {
      a.per_second == b.per_second && a.punctuation_pause == b.punctuation_pause && a.fade == b.fade
    }
    _ => false,
  }
}

/// A root's placement, written when the tree moved it; the placement as the
/// tree now gives it
fn update_place(
  node: &mut EntityWorldMut<'_>,
  applied: &UiApplied,
  modal: bool,
  embedded: bool,
) -> Option<(Anchor, glam::Vec2, Option<crate::core::ui::Rect>)> {
  let (layout_anchor, layout_offset, at) = {
    let kind = node.get::<UiNodeKind>()?;
    (kind.layout.anchor, kind.layout.offset, kind.layout.at)
  };
  let mut root = node.get_mut::<UiRoot>()?;
  let default = if modal {
    Anchor::Center
  } else {
    Anchor::TopLeft
  };
  let place = (
    layout_anchor.unwrap_or(default),
    layout_offset.unwrap_or(glam::Vec2::ZERO),
    at,
  );
  if applied.place != Some(place) {
    root.anchor = place.0;
    root.offset = place.1;
    root.at = place.2;
  }
  if root.embedded != embedded {
    root.embedded = embedded;
  }
  Some(place)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn one(h: &mut UiHarness, node: UiNode) -> Entity {
    h.spawn_ui(node)
  }

  fn update(h: &mut UiHarness, entity: Entity, node: UiNode) {
    let key = root_key(&node);
    update_node(h.world_mut(), entity, node, key);
  }

  #[test]
  fn a_tree_change_to_a_label_replaces_it_and_a_game_write_survives_an_unrelated_change() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::button("Play").width(100.0));
    h.world_mut().get_mut::<UiLabel>(e).unwrap().0 = "Game wrote".into();
    update(&mut h, e, UiNode::button("Play").width(140.0));
    assert_eq!(
      h.world().get::<UiLabel>(e).unwrap().0,
      "Game wrote",
      "the tree did not change the label"
    );
    let kind = h.world().get::<UiNodeKind>(e).unwrap();
    assert_eq!(
      kind.layout.width,
      Some(Length::Px(140.0)),
      "layout is the tree's"
    );
    update(&mut h, e, UiNode::button("Start").width(140.0));
    assert_eq!(
      h.world().get::<UiLabel>(e).unwrap().0,
      "Start",
      "the tree changed the label"
    );
  }

  #[test]
  fn a_player_moved_slider_survives_and_a_tree_value_change_wins() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::slider(0.0..=1.0, 0.5));
    h.world_mut().get_mut::<UiValue>(e).unwrap().0 = 0.9;
    update(&mut h, e, UiNode::slider(0.0..=2.0, 0.5));
    assert_eq!(h.world().get::<UiValue>(e).unwrap().0, 0.9);
    update(&mut h, e, UiNode::slider(0.0..=2.0, 1.5));
    assert_eq!(h.world().get::<UiValue>(e).unwrap().0, 1.5);
  }

  #[test]
  fn a_bound_value_is_never_written_by_an_update() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::slider(0.0..=1.0, 0.5).bind("v"));
    update(&mut h, e, UiNode::slider(0.0..=1.0, 0.2).bind("v"));
    assert_eq!(
      h.world().get::<UiValue>(e).unwrap().0,
      0.5,
      "the binding owns the value"
    );
  }

  #[test]
  fn a_changed_pattern_formats_again_and_an_unchanged_one_keeps_the_label() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::text("Score {s}").bound());
    h.ui_set(e, "s", 3);
    update(&mut h, e, UiNode::text("Score {s}").bound().size(20.0));
    assert_eq!(h.world().get::<UiLabel>(e).unwrap().0, "Score 3");
    update(&mut h, e, UiNode::text("Points {s}").bound());
    assert_eq!(h.world().get::<UiLabel>(e).unwrap().0, "Points 3");
  }

  #[test]
  fn hidden_disabled_tooltip_and_name_follow_the_rules() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::button("A").tooltip("tip").name("a"));
    h.world_mut().entity_mut(e).insert(UiHidden); // the game hid it
    update(
      &mut h,
      e,
      UiNode::button("A").tooltip("tip").name("b").enabled(false),
    );
    assert!(
      h.world().get::<UiHidden>(e).is_some(),
      "the tree did not change hidden"
    );
    assert!(
      h.world().get::<UiDisabled>(e).is_some(),
      "the tree disabled it"
    );
    assert_eq!(
      h.world().get::<UiName>(e).unwrap().0,
      "b",
      "names are the tree's"
    );
    update(&mut h, e, UiNode::button("A").name("b").enabled(false));
    assert!(
      h.world().get::<UiTooltip>(e).is_none(),
      "the tree removed the tooltip"
    );
  }

  #[test]
  fn a_root_moved_by_the_game_stays_until_the_tree_moves_it() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(
      &mut h,
      UiNode::column().anchor(Anchor::TopLeft).offset(10.0, 10.0),
    );
    h.world_mut().get_mut::<UiRoot>(e).unwrap().offset.y = 99.0;
    update(
      &mut h,
      e,
      UiNode::column()
        .anchor(Anchor::TopLeft)
        .offset(10.0, 10.0)
        .gap(4.0),
    );
    assert_eq!(h.world().get::<UiRoot>(e).unwrap().offset.y, 99.0);
    update(
      &mut h,
      e,
      UiNode::column().anchor(Anchor::TopLeft).offset(10.0, 20.0),
    );
    assert_eq!(h.world().get::<UiRoot>(e).unwrap().offset.y, 20.0);
  }

  #[test]
  fn an_unchanged_update_writes_nothing() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(
      &mut h,
      UiNode::checkbox("Mute", true).tooltip("t").name("m"),
    );
    h.world_mut().clear_trackers();
    update(
      &mut h,
      e,
      UiNode::checkbox("Mute", true).tooltip("t").name("m"),
    );
    let entity = h.world().entity(e);
    assert!(!entity.get_ref::<UiNodeKind>().unwrap().is_changed());
    assert!(!entity.get_ref::<UiChecked>().unwrap().is_changed());
    assert!(!entity.get_ref::<UiLabel>().unwrap().is_changed());
    assert!(!entity.get_ref::<UiName>().unwrap().is_changed());
  }

  #[test]
  fn the_same_kind_means_the_same_constructor() {
    assert!(same_kind(
      &UiNode::slider(0.0..=1.0, 0.0).kind,
      &UiNode::slider(0.0..=9.0, 3.0).kind
    ));
    assert!(!same_kind(
      &UiNode::checkbox("a", true).kind,
      &UiNode::toggle("a", true).kind
    ));
  }
}
