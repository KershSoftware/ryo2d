//! Bringing a spawned tree in line with a new one: each node updated in
//! place, children matched by key

use std::collections::{HashMap, VecDeque};

use crate::core::ecs::kernel::{
  ChildOf, Children, Component, DetectChangesMut, Entity, EntityWorldMut, World,
};
use crate::core::text::TextReveal;
use crate::core::ui::Anchor;
use crate::core::ui::theme::warn_once;

use super::bindings::{self, Pattern, UiBind, UiBindings, UiPattern};
use super::components::{
  UiAdded, UiApplied, UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiLabel, UiName, UiNodes,
  UiOptions, UiRoot, UiTextValue, UiTooltip, UiTreeNode, UiValue,
};
use super::node::{Kind, NodeState, UiNode, UiNodeKind};
use super::spawn::{
  Keys, Pending, child_keys, finish, root_key, root_of, spawn_node, spawn_root, warn_repeated_keys,
};

/// Which children of a node an update may touch
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
  /// `ctx.update_ui`: every child the tree spawned
  Update,
  /// A file reload: only the children built from the same file instance
  #[allow(dead_code)] // built by the UI files' hot reload, which comes later
  Reload { instance: u64 },
}

/// `ctx.update_ui`: reconcile a node spawned from a tree; any other entity
/// warns once and is returned as it is
pub(crate) fn update_ui(world: &mut World, node: Entity, tree: UiNode) -> Entity {
  if world.get::<UiTreeNode>(node).is_none() {
    warn_once("update-ui:not-a-tree-node".to_string(), || {
      "update_ui needs a node spawned from a UiNode tree".to_string()
    });
    return node;
  }
  reconcile(world, node, tree, Mode::Update)
}

/// Bring `entity`'s spawned tree in line with `tree`; the entity, or its
/// replacement if `tree`'s top is a different kind
pub(crate) fn reconcile(world: &mut World, entity: Entity, mut tree: UiNode, mode: Mode) -> Entity {
  if world.get_entity(entity).is_err() {
    return entity;
  }
  // Only the top's seeds count, as at spawn
  let seeds = std::mem::take(&mut tree.seeds);
  // The top keeps the key its own parent matches it by
  let key = world
    .get::<UiTreeNode>(entity)
    .is_none()
    .then(|| root_key(&tree));
  let mut walk = Walk {
    mode,
    pending: Vec::new(),
    fresh: Vec::new(),
    stack: Vec::new(),
    nth: Vec::new(),
  };
  let top = walk.node(world, entity, tree, key);
  finish(world, top, walk.pending);
  bindings::invalidate(world, top);
  bindings::seed(world, top, seeds);
  for node in walk.fresh {
    bindings::apply_subtree(world, node, Vec::new());
  }
  top
}

/// One reconcile: what it leaves for the end, and scratch reused down the
/// tree, so an unchanged tree allocates per walk rather than per node
struct Walk {
  mode: Mode,
  pending: Vec<Pending>,
  /// Subtrees spawned, and nodes rebound: their bindings start fresh
  fresh: Vec<Entity>,
  /// The children lists being walked, outermost first
  stack: Vec<Entity>,
  /// Scratch for the keys of one list of tree children
  nth: Vec<usize>,
}

impl Walk {
  /// Bring `entity` in line with `tree`; `key` is its new key, or `None` to
  /// keep its own
  fn node(
    &mut self,
    world: &mut World,
    entity: Entity,
    tree: UiNode,
    key: Option<String>,
  ) -> Entity {
    let same = world
      .get::<UiNodeKind>(entity)
      .is_some_and(|node| same_kind(&node.kind, &tree.kind));
    if !same {
      let key = key
        .or_else(|| world.get::<UiTreeNode>(entity).map(|node| node.key.clone()))
        .unwrap_or_else(|| root_key(&tree));
      let new = respawn_in_place(world, entity, tree, key, &mut self.pending);
      self.fresh.push(new);
      return new;
    }
    let NodeUpdate {
      children,
      nav,
      popup,
      modal,
      rebound,
    } = update_node(world, entity, tree, key);
    if rebound {
      self.fresh.push(entity);
    }
    self.pending.push(Pending {
      entity,
      nav,
      popup,
      modal,
    });
    self.children(world, entity, children);
    entity
  }

  /// Bring the children of `parent` the tree owns in line with `children`;
  /// the others stay where they were relative to the owned ones
  fn children(&mut self, world: &mut World, parent: Entity, children: Vec<UiNode>) {
    let start = self.stack.len();
    if let Some(kids) = world.get::<Children>(parent) {
      self.stack.extend(kids.iter());
    }
    if self.stack.len() == start && children.is_empty() {
      return;
    }
    if self.in_place(world, start, &children) {
      // Nothing is spawned, despawned or moved: each owned child in turn is
      // updated (a kind change respawns it where it was). Deeper lists use
      // the stack above `end` and leave it as they found it.
      let (mode, end) = (self.mode, self.stack.len());
      let mut at = start;
      for child in children {
        while at < end && !owns(world, self.stack[at], mode) {
          at += 1;
        }
        if at == end {
          break;
        }
        let entity = self.stack[at];
        at += 1;
        self.node(world, entity, child, None);
      }
      self.stack.truncate(start);
    } else {
      let current = self.stack.split_off(start);
      self.rematch(world, parent, current, children);
    }
  }

  /// Whether the owned children in `stack[start..]` already have the tree's
  /// keys, in the tree's order
  fn in_place(&mut self, world: &World, start: usize, children: &[UiNode]) -> bool {
    let mode = self.mode;
    let mut keys = Keys::new(children, &mut self.nth);
    let mut owned = self.stack[start..]
      .iter()
      .filter(|&&child| owns(world, child, mode));
    loop {
      match (keys.next(), owned.next()) {
        (None, None) => return true,
        (Some(key), Some(&child))
          if world
            .get::<UiTreeNode>(child)
            .is_some_and(|node| key.is(&node.key)) => {}
        _ => return false,
      }
    }
  }

  /// Match the tree's children to the owned `current` ones by key, spawn the
  /// new and despawn the missing, and put them in the tree's order, each kept
  /// child after the owned sibling it followed
  fn rematch(
    &mut self,
    world: &mut World,
    parent: Entity,
    current: Vec<Entity>,
    children: Vec<UiNode>,
  ) {
    // The owned children in order, and each kept one with the index of the
    // owned sibling it followed
    let mut owned = Vec::with_capacity(current.len());
    let mut kept = Vec::new();
    for &child in &current {
      if owns(world, child, self.mode) {
        owned.push(child);
      } else {
        kept.push((child, owned.len().checked_sub(1)));
      }
    }

    let keys = child_keys(&children);
    warn_repeated_keys(&keys);
    let mut by_key: HashMap<&str, VecDeque<usize>> = HashMap::with_capacity(owned.len());
    for (i, &child) in owned.iter().enumerate() {
      if let Some(node) = world.get::<UiTreeNode>(child) {
        by_key.entry(node.key.as_str()).or_default().push_back(i);
      }
    }
    let matched: Vec<Option<usize>> = keys
      .iter()
      .map(|key| by_key.get_mut(key.as_str()).and_then(VecDeque::pop_front))
      .collect();

    // What each owned child became: itself, its replacement, or nothing
    let mut became = vec![None; owned.len()];
    let mut order = Vec::with_capacity(children.len() + kept.len());
    for ((child, key), slot) in children.into_iter().zip(keys).zip(matched) {
      let entity = match slot {
        Some(i) => {
          let entity = self.node(world, owned[i], child, Some(key));
          became[i] = Some(entity);
          entity
        }
        None => {
          let entity = spawn_node(world, Some(parent), false, child, key, &mut self.pending);
          self.fresh.push(entity);
          entity
        }
      };
      order.push(entity);
    }
    for (&child, became) in owned.iter().zip(&became) {
      if became.is_none() && world.get_entity(child).is_ok() {
        world.despawn(child);
      }
    }

    if !kept.is_empty() {
      // After the owned sibling it followed, else the nearest earlier one
      // that survived, else first
      let after = |i: Option<usize>| i.and_then(|i| became[..=i].iter().rev().find_map(|e| *e));
      let kept: Vec<(Entity, Option<Entity>)> = kept.iter().map(|&(e, i)| (e, after(i))).collect();
      let mut placed = Vec::with_capacity(order.len() + kept.len());
      placed.extend(kept.iter().filter(|(_, at)| at.is_none()).map(|(e, _)| *e));
      for &entity in &order {
        placed.push(entity);
        placed.extend(
          kept
            .iter()
            .filter(|(_, at)| *at == Some(entity))
            .map(|(e, _)| *e),
        );
      }
      order = placed;
    }
    if world
      .get::<Children>(parent)
      .is_some_and(|kids| **kids != *order)
    {
      world.entity_mut(parent).replace_children(&order);
    }
  }
}

/// Whether an update in `mode` may touch `child`: spawned from a tree, not
/// added with `spawn_ui_in`, and in a reload from the same file instance
fn owns(world: &World, child: Entity, mode: Mode) -> bool {
  let Ok(child) = world.get_entity(child) else {
    return false;
  };
  if child.contains::<UiAdded>() {
    return false;
  }
  match (child.get::<UiTreeNode>(), mode) {
    (None, _) => false,
    (Some(_), Mode::Update) => true,
    (Some(node), Mode::Reload { instance }) => node.instance == instance,
  }
}

/// Despawn `entity` and spawn `tree` where it was: at the same place among
/// its parent's children, or for a root in the same scene and slot of
/// `UiNodes::roots` (so it draws in the same order), keeping its values
fn respawn_in_place(
  world: &mut World,
  entity: Entity,
  tree: UiNode,
  key: String,
  pending: &mut Vec<Pending>,
) -> Entity {
  let added = world.get::<UiAdded>(entity).is_some();
  let new = match world
    .get::<UiRoot>(entity)
    .map(|root| (root.scene, root.seq))
  {
    Some((scene, seq)) => {
      let values = world.entity_mut(entity).take::<UiBindings>();
      world.despawn(entity);
      let new = spawn_root(world, scene, seq, tree, key, pending);
      if let Some(values) = values {
        world.entity_mut(new).insert(values);
      }
      let mut nodes = world.get_resource_or_insert_with(UiNodes::default);
      match nodes.roots.iter_mut().find(|(_, root)| *root == entity) {
        Some(slot) => slot.1 = new,
        None => nodes.roots.push((scene, new)),
      }
      new
    }
    None => {
      let parent = world.get::<ChildOf>(entity).map(ChildOf::parent);
      let index = parent
        .and_then(|parent| world.get::<Children>(parent))
        .and_then(|kids| kids.iter().position(|&kid| kid == entity));
      world.despawn(entity);
      let new = spawn_node(world, parent, false, tree, key, pending);
      if let (Some(parent), Some(index)) = (parent, index) {
        world.entity_mut(parent).insert_child(index, new);
      }
      new
    }
  };
  if added {
    world.entity_mut(new).insert(UiAdded);
  }
  new
}

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
/// An unchanged node allocates and writes nothing. `key` is the node's new
/// key, or `None` to keep its own.
pub(crate) fn update_node(
  world: &mut World,
  entity: Entity,
  node: UiNode,
  key: Option<String>,
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
  // Taken rather than cloned; replaced below by what the tree gives now
  let Some(applied) = node
    .get_mut::<UiApplied>()
    .map(|mut applied| std::mem::take(applied.bypass_change_detection()))
  else {
    return update;
  };

  update_settings(
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
    key,
    instance,
  );
  update.rebound = set(&mut node, bind);
  let patterns = update_patterns(
    &mut node,
    &applied,
    bound,
    state.label.as_deref(),
    tooltip.as_deref(),
  );

  let has_bind = node.contains::<UiBind>();
  update_state(&mut node, &applied, &state, has_bind, patterns);
  if patterns.tooltip_dropped {
    // The formatted text goes with its pattern: the tree's text is back
    set(&mut node, tooltip.clone().map(UiTooltip));
  } else if !patterns.tooltip && tooltip != applied.tooltip {
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
    *stored.bypass_change_detection() = UiApplied {
      state,
      tooltip,
      hidden,
      disabled,
      place,
    };
  }

  if update.rebound || patterns.changed {
    bindings::invalidate(world, entity);
  }
  if patterns.changed
    && let Some(root) = root_of(world, entity)
  {
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

/// Rewrite what the tree owns where it differs
fn update_settings(
  node: &mut EntityWorldMut<'_>,
  new: UiNodeKind,
  name: Option<UiName>,
  key: Option<String>,
  instance: u64,
) {
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
  set(node, name);
  match key {
    Some(key) => {
      set(node, Some(UiTreeNode { key, instance }));
    }
    None => {
      if let Some(mut tree) = node.get_mut::<UiTreeNode>()
        && tree.instance != instance
      {
        tree.instance = instance;
      }
    }
  }
}

/// Which of a node's label and tooltip have patterns after an update
#[derive(Clone, Copy)]
struct Patterns {
  label: bool,
  tooltip: bool,
  /// The tree took away the label's pattern
  label_dropped: bool,
  /// The tree took away the tooltip's pattern
  tooltip_dropped: bool,
  /// A pattern was added, removed or given new text
  changed: bool,
}

/// Make the node's patterns the tree's, parsing one only when its text changed
fn update_patterns(
  node: &mut EntityWorldMut<'_>,
  applied: &UiApplied,
  bound: bool,
  label: Option<&str>,
  tooltip: Option<&str>,
) -> Patterns {
  let (had_label, had_tooltip) = node
    .get::<UiPattern>()
    .map_or((false, false), |p| (p.label.is_some(), p.tooltip.is_some()));
  let has_label = bound && label.is_some();
  let has_tooltip = bound && tooltip.is_some();
  // A pattern's text is what the tree gave last time
  let parse_label = has_label && (!had_label || label != applied.state.label.as_deref());
  let parse_tooltip = has_tooltip && (!had_tooltip || tooltip != applied.tooltip.as_deref());
  let changed =
    parse_label || parse_tooltip || had_label != has_label || had_tooltip != has_tooltip;
  if changed {
    let old = node
      .get_mut::<UiPattern>()
      .map(|mut p| std::mem::take(p.bypass_change_detection()))
      .unwrap_or_default();
    let pattern = UiPattern {
      label: match parse_label {
        true => label.map(Pattern::parse),
        false => old.label.filter(|_| has_label),
      },
      tooltip: match parse_tooltip {
        true => tooltip.map(Pattern::parse),
        false => old.tooltip.filter(|_| has_tooltip),
      },
    };
    write(
      node,
      Some(pattern).filter(|p| p.label.is_some() || p.tooltip.is_some()),
    );
    // A new label pattern formats into the label, which must exist
    if let Some(label) = label.filter(|_| parse_label)
      && !node.contains::<UiLabel>()
    {
      node.insert(UiLabel(label.to_string()));
    }
  }
  Patterns {
    label: has_label,
    tooltip: has_tooltip,
    label_dropped: had_label && !has_label,
    tooltip_dropped: had_tooltip && !has_tooltip,
    changed,
  }
}

/// The three-way rule for each state component: write only where the tree
/// changed what it gave
fn update_state(
  node: &mut EntityWorldMut<'_>,
  applied: &UiApplied,
  state: &NodeState,
  bound: bool,
  patterns: Patterns,
) {
  let old = &applied.state;
  if patterns.label_dropped {
    // The formatted text goes with its pattern: the tree's text is back
    set(node, state.label.clone().map(UiLabel));
  } else if !patterns.label && state.label != old.label {
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
  use crate::prelude::*;

  fn one(h: &mut UiHarness, node: UiNode) -> Entity {
    h.spawn_ui(node)
  }

  fn update(h: &mut UiHarness, entity: Entity, node: UiNode) {
    let key = root_key(&node);
    update_node(h.world_mut(), entity, node, Some(key));
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
  fn a_dropped_pattern_shows_the_trees_text_again() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(
      &mut h,
      UiNode::button("Score {s}").tooltip("Tip {s}").bound(),
    );
    h.ui_set(e, "s", 3);
    assert_eq!(h.world().get::<UiLabel>(e).unwrap().0, "Score 3");
    update(&mut h, e, UiNode::button("Score {s}").tooltip("Tip {s}"));
    assert_eq!(h.world().get::<UiLabel>(e).unwrap().0, "Score {s}");
    assert_eq!(h.world().get::<UiTooltip>(e).unwrap().0, "Tip {s}");
    h.ui_set(e, "s", 4);
    assert_eq!(
      h.world().get::<UiLabel>(e).unwrap().0,
      "Score {s}",
      "no longer formatted"
    );
  }

  #[test]
  fn a_pattern_added_by_an_update_formats_the_label() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::text("Score {s}"));
    h.ui_set(e, "s", 3);
    update(&mut h, e, UiNode::text("Score {s}").bound());
    assert_eq!(h.world().get::<UiLabel>(e).unwrap().0, "Score 3");
  }

  #[test]
  fn rebound_is_set_when_bind_is_added_removed_or_renamed() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::slider(0.0..=1.0, 0.5));
    let mut rebound = |node: UiNode| {
      let key = root_key(&node);
      update_node(h.world_mut(), e, node, Some(key)).rebound
    };
    let slider = || UiNode::slider(0.0..=1.0, 0.5);
    assert!(!rebound(slider()), "no bind before or after");
    assert!(rebound(slider().bind("v")), "added");
    assert!(!rebound(slider().bind("v")), "the same name");
    assert!(rebound(slider().bind("w")), "renamed");
    assert!(rebound(slider()), "removed");
  }

  #[test]
  fn an_unchanged_update_of_a_bound_node_writes_nothing() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::checkbox("On {n}", true).bind("on").bound());
    h.ui_set(e, "n", 2);
    h.world_mut().clear_trackers();
    update(
      &mut h,
      e,
      UiNode::checkbox("On {n}", true).bind("on").bound(),
    );
    let entity = h.world().entity(e);
    assert!(!entity.get_ref::<UiNodeKind>().unwrap().is_changed());
    assert!(!entity.get_ref::<UiLabel>().unwrap().is_changed());
    assert!(!entity.get_ref::<UiChecked>().unwrap().is_changed());
    assert_eq!(entity.get::<UiLabel>().unwrap().0, "On 2");
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

#[cfg(test)]
mod tree_tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn kids(h: &UiHarness, of: Entity) -> Vec<Entity> {
    h.world()
      .get::<Children>(of)
      .map(|c| c.to_vec())
      .unwrap_or_default()
  }

  fn menu(labels: &[&str]) -> UiNode {
    UiNode::column()
      .focus_scope()
      .children(labels.iter().map(|l| UiNode::button(*l).name(*l)))
  }

  #[test]
  fn keyed_children_keep_their_entities_through_add_remove_and_reorder() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(menu(&["a", "b", "c"]));
    let before = kids(&h, root);
    let same = h.update_ui(root, menu(&["c", "a", "d"]));
    assert_eq!(same, root);
    let after = kids(&h, root);
    assert_eq!(after[0], before[2], "c moved first, same entity");
    assert_eq!(after[1], before[0], "a kept");
    assert!(!before.contains(&after[2]), "d is new");
    assert!(h.world().get_entity(before[1]).is_err(), "b despawned");
  }

  #[test]
  fn a_kind_change_respawns_in_place() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::button("x").key("k"))
        .child(UiNode::text("after")),
    );
    let old = kids(&h, root)[0];
    h.update_ui(
      root,
      UiNode::column()
        .child(UiNode::checkbox("x", true).key("k"))
        .child(UiNode::text("after")),
    );
    let now = kids(&h, root);
    assert_ne!(now[0], old);
    assert_eq!(
      h.world().get::<UiNodeKind>(now[0]).unwrap().name(),
      "checkbox"
    );
    assert_eq!(now.len(), 2);
  }

  #[test]
  fn a_root_kind_change_returns_the_new_root_in_the_same_draw_slot() {
    let mut h = UiHarness::new(400.0, 300.0);
    let first = h.spawn_ui(UiNode::column());
    let _second = h.spawn_ui(UiNode::column());
    let new = h.update_ui(first, UiNode::row());
    assert_ne!(new, first);
    let roots = &h.world().resource::<UiNodes>().roots;
    assert_eq!(roots[0].1, new, "drawn where the old root was");
  }

  #[test]
  fn the_games_children_and_components_stay() {
    let mut h = UiHarness::new(400.0, 300.0);
    #[derive(Component)]
    struct Mine;
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("head"))
        .child(UiNode::column().name("extras")),
    );
    let extras = h.find_ui("extras").unwrap();
    let added = spawn_ui_in(h.world_mut(), extras, UiNode::button("game's")).unwrap();
    let head = kids(&h, root)[0];
    h.world_mut().entity_mut(head).insert(Mine);
    h.update_ui(
      root,
      UiNode::column()
        .gap(3.0)
        .child(UiNode::text("head"))
        .child(UiNode::column().name("extras")),
    );
    assert_eq!(
      kids(&h, extras),
      [added],
      "spawn_ui_in subtrees are the game's"
    );
    assert!(
      h.world().get::<Mine>(head).is_some(),
      "inserted components stay"
    );
  }

  #[test]
  fn a_kept_child_stays_after_the_sibling_it_followed() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(menu(&["a", "b"]));
    let added = spawn_ui_in(h.world_mut(), root, UiNode::text("game's")).unwrap();
    // move the game's node between a and b
    let a = kids(&h, root)[0];
    h.world_mut().entity_mut(root).insert_children(1, &[added]);
    h.update_ui(root, menu(&["a", "z", "b"]));
    let now = kids(&h, root);
    assert_eq!(now[0], a);
    assert_eq!(now[1], added, "still right after a");
  }

  #[test]
  fn focus_and_ids_survive_an_update_and_nothing_is_sent() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(menu(&["a", "b"]));
    h.frame(|_| {});
    h.action("ui.down");
    h.frame(|_| {});
    let focused = h.focused();
    h.update_ui(root, menu(&["a", "b"]).gap(10.0));
    h.frame(|_| {});
    assert_eq!(h.focused(), focused);
    assert!(h.events().is_empty(), "a reconcile sends no UiEvents");
  }

  #[test]
  fn nav_names_resolve_again_after_an_update() {
    let mut h = UiHarness::new(400.0, 300.0);
    let tree = |target: &str| {
      UiNode::column()
        .child(UiNode::button("a").name("a").nav_down(target))
        .child(UiNode::button("b").name("b"))
        .child(UiNode::button("c").name("c"))
    };
    let root = h.spawn_ui(tree("b"));
    h.update_ui(root, tree("c"));
    let a = h.find_ui("a").unwrap();
    let c = h.find_ui("c").unwrap();
    assert_eq!(h.world().get::<UiNodeKind>(a).unwrap().nav[1], Some(c));
  }

  #[test]
  fn reload_mode_leaves_other_instances_alone() {
    let mut h = UiHarness::new(400.0, 300.0);
    let mut file_child = UiNode::text("from the file");
    file_child.instance = 7;
    let mut code_child = UiNode::text("from code").key("code");
    code_child.instance = 0;
    let mut top = UiNode::column().child(file_child).child(code_child);
    top.instance = 7;
    let root = h.spawn_ui(top);
    let mut again = UiNode::column();
    again.instance = 7;
    reconcile(h.world_mut(), root, again, Mode::Reload { instance: 7 });
    let now = kids(&h, root);
    assert_eq!(now.len(), 1, "the file's child went; the code child stayed");
    assert_eq!(h.world().get::<UiLabel>(now[0]).unwrap().0, "from code");
  }

  #[test]
  fn a_label_the_game_wrote_on_a_bound_node_survives_an_unrelated_change() {
    let mut h = UiHarness::new(400.0, 300.0);
    let tree = |size: f32| {
      UiNode::column().child(UiNode::text("Score {s}").bound().size(size).name("score"))
    };
    let root = h.spawn_ui(tree(16.0));
    h.ui_set(root, "s", 3);
    let score = h.find_ui("score").unwrap();
    h.world_mut().get_mut::<UiLabel>(score).unwrap().0 = "Game wrote".into();
    h.update_ui(root, tree(20.0));
    assert_eq!(h.world().get::<UiLabel>(score).unwrap().0, "Game wrote");
    h.ui_set(root, "s", 4);
    assert_eq!(
      h.world().get::<UiLabel>(score).unwrap().0,
      "Score 4",
      "still formatted from the binding"
    );
  }

  #[test]
  fn an_unchanged_update_leaves_change_detection_alone() {
    let mut h = UiHarness::new(400.0, 300.0);
    let tree = || {
      UiNode::column()
        .child(UiNode::button("a").name("a").nav_down("b"))
        .child(UiNode::button("b").name("b"))
    };
    let root = h.spawn_ui(tree());
    h.world_mut().clear_trackers();
    h.update_ui(root, tree());
    let w = h.world();
    assert!(!w.entity(root).get_ref::<Children>().unwrap().is_changed());
    for node in [root, kids(&h, root)[0], kids(&h, root)[1]] {
      assert!(!w.entity(node).get_ref::<UiNodeKind>().unwrap().is_changed());
    }
    for node in kids(&h, root) {
      assert!(!w.entity(node).get_ref::<UiLabel>().unwrap().is_changed());
    }
  }

  #[test]
  fn the_top_seeds_fill_only_names_without_a_value() {
    let mut h = UiHarness::new(400.0, 300.0);
    let tree = |seed: i32| {
      UiNode::column()
        .child(UiNode::text("{s}").bound().name("t"))
        .seed("s", BindValue::from(seed))
    };
    let root = h.spawn_ui(UiNode::column().child(UiNode::text("{s}").bound().name("t")));
    h.update_ui(root, tree(1));
    let t = h.find_ui("t").unwrap();
    assert_eq!(h.world().get::<UiLabel>(t).unwrap().0, "1");
    h.ui_set(root, "s", 2);
    h.update_ui(root, tree(9));
    assert_eq!(h.world().get::<UiLabel>(t).unwrap().0, "2");
  }

  #[test]
  fn spawned_and_rebound_widgets_join_the_roots_values() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::progress(0.7).bind("v").name("bar"))
        .child(UiNode::slider(0.0..=1.0, 0.2).name("slider")),
    );
    h.update_ui(
      root,
      UiNode::column()
        .child(UiNode::progress(0.7).bind("v").name("bar"))
        .child(UiNode::slider(0.0..=1.0, 0.2).bind("v").name("slider"))
        .child(UiNode::progress(0.0).bind("v").name("new")),
    );
    let slider = h.find_ui("slider").unwrap();
    let new = h.find_ui("new").unwrap();
    assert_eq!(h.world().get::<UiValue>(slider).unwrap().0, 0.7, "rebound");
    assert_eq!(h.world().get::<UiValue>(new).unwrap().0, 0.7, "spawned");
  }

  #[test]
  fn a_top_of_another_kind_takes_its_place_and_a_root_keeps_its_values() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("{s}").bound())
        .child(UiNode::button("x").name("x"))
        .child(UiNode::text("after")),
    );
    h.ui_set(root, "s", 5);
    let x = h.find_ui("x").unwrap();
    let new = h.update_ui(x, UiNode::checkbox("x", true).name("x"));
    assert_ne!(new, x);
    assert_eq!(kids(&h, root)[1], new, "the same place among its siblings");

    let row = h.update_ui(root, UiNode::row().child(UiNode::text("{s}").bound()));
    assert_ne!(row, root);
    assert_eq!(h.ui_get::<i32>(row, "s"), Some(5));
    assert_eq!(h.world().get::<UiLabel>(kids(&h, row)[0]).unwrap().0, "5");
  }

  #[test]
  fn repeated_keys_match_in_order() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::button("1").key("same"))
        .child(UiNode::button("2").key("same")),
    );
    let before = kids(&h, root);
    h.update_ui(
      root,
      UiNode::column()
        .child(UiNode::text("new"))
        .child(UiNode::button("1").key("same"))
        .child(UiNode::button("2").key("same")),
    );
    assert_eq!(kids(&h, root)[1..], before[..]);
  }

  #[test]
  fn update_ui_on_a_plain_entity_warns_and_returns_it() {
    let mut h = UiHarness::new(400.0, 300.0);
    let plain = h.world_mut().spawn_empty().id();
    assert_eq!(h.update_ui(plain, UiNode::column()), plain);
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("update_ui needs a node"),
      1
    );
  }
}
