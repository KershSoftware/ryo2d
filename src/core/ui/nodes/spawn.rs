//! Spawning a node tree as entities, finding nodes by name, and despawning a scene's trees

use std::collections::HashSet;
use std::fmt;

use glam::Vec2;

use crate::core::ecs::kernel::{ChildOf, Children, Entity, World};
use crate::core::ui::Anchor;
use crate::core::ui::theme::warn_once;

use super::bindings::{self, Pattern, UiBind, UiPattern};
use super::components::{
  BackAnswer, UiAdded, UiApplied, UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiInteraction,
  UiLabel, UiName, UiNodes, UiOptions, UiRoot, UiSource, UiTextValue, UiTooltip, UiTreeNode,
  UiValue,
};
use super::list::{self, UiList, UiListRow, UiListShape};
use super::node::{Kind, UiNode, UiNodeKind};

const NAV_OPTIONS: [&str; 4] = ["nav_up", "nav_down", "nav_left", "nav_right"];

/// What a spawned or updated node still needs once its whole tree exists
pub(crate) struct Pending {
  pub(crate) entity: Entity,
  pub(crate) nav: [Option<String>; 4],
  pub(crate) popup: Option<String>,
  pub(crate) modal: bool,
}

/// A child's key, borrowed from its tree rather than written out
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key<'a> {
  /// Its `key`, else its name
  Given(&'a str),
  /// Its label (the template, else the kind) and its index among the
  /// siblings of that label that have neither
  Nth(&'a str, usize),
}

impl Key<'_> {
  /// Whether `key` is this key written out
  pub(crate) fn is(self, key: &str) -> bool {
    match self {
      Key::Given(given) => given == key,
      Key::Nth(label, n) => key
        .strip_prefix(label)
        .and_then(|rest| rest.strip_prefix('#'))
        .is_some_and(|digits| is_decimal(digits, n)),
    }
  }
}

/// Whether `digits` is `n` written in decimal, as `Display` writes it
fn is_decimal(digits: &str, n: usize) -> bool {
  digits.bytes().all(|b| b.is_ascii_digit())
    && (digits.len() == 1 || !digits.starts_with('0'))
    && digits.parse() == Ok(n)
}

impl fmt::Display for Key<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Key::Given(key) => f.write_str(key),
      Key::Nth(label, n) => write!(f, "{label}#{n}"),
    }
  }
}

/// The keys of a node's children in order; `nth` is scratch, kept by the
/// caller so walking many lists allocates once
pub(crate) struct Keys<'a, 's> {
  children: &'a [UiNode],
  nth: &'s mut Vec<usize>,
}

impl<'a, 's> Keys<'a, 's> {
  pub(crate) fn new(children: &'a [UiNode], nth: &'s mut Vec<usize>) -> Keys<'a, 's> {
    nth.clear();
    Keys { children, nth }
  }
}

impl<'a> Iterator for Keys<'a, '_> {
  type Item = Key<'a>;

  fn next(&mut self) -> Option<Key<'a>> {
    let children = self.children;
    let i = self.nth.len();
    let child = children.get(i)?;
    let given = |node: &'a UiNode| node.key.as_deref().or(node.name.as_deref());
    let label = |node: &'a UiNode| node.template.as_deref().unwrap_or(node.kind.name());
    if let Some(key) = given(child) {
      self.nth.push(0);
      return Some(Key::Given(key));
    }
    let own = label(child);
    // One more than the nearest earlier sibling of the label with neither
    let n = (0..i)
      .rev()
      .find(|&j| given(&children[j]).is_none() && label(&children[j]) == own)
      .map_or(0, |j| self.nth[j] + 1);
    self.nth.push(n);
    Some(Key::Nth(own, n))
  }
}

/// The keys of a node's children: a child's key, else its name, else its
/// label (the template, else the kind) and its index among the siblings of
/// that label that have neither
pub(crate) fn child_keys(children: &[UiNode]) -> Vec<String> {
  Keys::new(children, &mut Vec::new())
    .map(|key| key.to_string())
    .collect()
}

/// Warn once for each key two siblings share
pub(crate) fn warn_repeated_keys(keys: &[String]) {
  if keys.len() < 2 {
    return;
  }
  let mut seen = HashSet::with_capacity(keys.len());
  for key in keys {
    if !seen.insert(key.as_str()) {
      warn_once(format!("ui-key-twice:{key}"), || {
        format!("two UI nodes under one parent are keyed \"{key}\"; they match in order")
      });
    }
  }
}

/// The key of a tree's top node
pub(crate) fn root_key(node: &UiNode) -> String {
  node
    .key
    .as_ref()
    .or(node.name.as_ref())
    .cloned()
    .unwrap_or_else(|| format!("{}#0", node.template.as_deref().unwrap_or(node.kind.name())))
}

/// Spawn `tree` as a root owned by `scene`
pub(crate) fn spawn_ui(world: &mut World, scene: u64, mut tree: UiNode) -> Entity {
  let seeds = std::mem::take(&mut tree.seeds);
  let mut pending = Vec::new();
  let key = root_key(&tree);
  let mut nodes = world.get_resource_or_insert_with(UiNodes::default);
  let seq = nodes.seq;
  nodes.seq += 1;
  let top = spawn_root(world, scene, seq, tree, key, &mut pending);
  world
    .get_resource_or_insert_with(UiNodes::default)
    .roots
    .push((scene, top));

  finish(world, top, pending);
  bindings::invalidate(world, top);
  bindings::apply_subtree(world, top, seeds);
  top
}

/// Spawn `tree` as a root of `scene`, placed as it says; the caller lists it
/// in `UiNodes::roots` and finishes it
pub(crate) fn spawn_root(
  world: &mut World,
  scene: u64,
  seq: u64,
  tree: UiNode,
  key: String,
  pending: &mut Vec<Pending>,
) -> Entity {
  // A modal root opens centred, as `ui.modal()` does, unless told otherwise
  let default = if tree.flags.modal {
    Anchor::Center
  } else {
    Anchor::TopLeft
  };
  let anchor = tree.layout.anchor.unwrap_or(default);
  let offset = tree.layout.offset.unwrap_or(Vec2::ZERO);
  let (at, embedded) = (tree.layout.at, tree.embedded);
  let top = spawn_node(world, None, true, tree, key, pending);
  world.entity_mut(top).insert(UiRoot {
    anchor,
    offset,
    at,
    scene,
    seq,
    embedded,
  });
  if let Some(mut applied) = world.get_mut::<UiApplied>(top) {
    applied.place = Some((anchor, offset, at));
  }
  top
}

/// Spawn `tree` under `parent`, after its last child; `None` if `parent` is
/// not a container node
pub(crate) fn spawn_ui_in(world: &mut World, parent: Entity, mut tree: UiNode) -> Option<Entity> {
  let parent_kind = world.get::<UiNodeKind>(parent).map(|node| node.kind.name());
  let container = world
    .get::<UiNodeKind>(parent)
    .is_some_and(|node| node.kind.is_container());
  if !container {
    let what = match parent_kind {
      Some(kind) => format!("a {kind} node"),
      None => "an entity that is not a UI node".to_string(),
    };
    warn_once(format!("spawn-ui-in:{what}"), || {
      format!("spawn_ui_in needs a container node as its parent, not {what}")
    });
    return None;
  }
  let seeds = std::mem::take(&mut tree.seeds);
  let mut pending = Vec::new();
  let key = root_key(&tree);
  let top = spawn_node(world, Some(parent), false, tree, key, &mut pending);
  world.entity_mut(top).insert(UiAdded);
  finish(world, top, pending);
  bindings::invalidate(world, top);
  bindings::apply_subtree(world, top, seeds);
  Some(top)
}

/// Spawn `tree` as a pooled row of the list `list`, showing item `index`
/// from `frame` on: the list's own (a reconcile never matches it) and a
/// binding scope of its own
pub(crate) fn spawn_list_row(
  world: &mut World,
  list: Entity,
  mut tree: UiNode,
  index: usize,
  frame: u64,
) -> Entity {
  let seeds = std::mem::take(&mut tree.seeds);
  let mut pending = Vec::new();
  let key = root_key(&tree);
  let row = spawn_node(world, Some(list), false, tree, key, &mut pending);
  world.entity_mut(row).insert((
    UiAdded,
    UiListRow {
      index,
      bound_frame: frame,
    },
  ));
  finish(world, row, pending);
  bindings::apply_subtree(world, row, seeds);
  row
}

/// Spawn one node and, depth first, its children
pub(crate) fn spawn_node(
  world: &mut World,
  parent: Option<Entity>,
  is_root: bool,
  node: UiNode,
  key: String,
  pending: &mut Vec<Pending>,
) -> Entity {
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
    inserts,
    given,
    bind,
    bound,
    seeds: _,
    key: _,
    template: _,
    instance,
    source,
    row_shape,
  } = node;

  let kind_name = kind.name();
  for option in given {
    if !kind.takes(option) {
      warn_once(format!("node-option:{kind_name}:{option}"), || {
        format!("UiNode::{option} does nothing on a {kind_name} node")
      });
    }
  }
  if embedded && !is_root {
    warn_once("node-option:embedded:child".to_string(), || {
      "UiNode::embedded does nothing on a child node".to_string()
    });
  }

  let child_keys = child_keys(&children);
  warn_repeated_keys(&child_keys);
  let applied = UiApplied {
    state: state.clone(),
    tooltip: tooltip.clone(),
    hidden,
    disabled,
    place: None,
  };

  let popup = match &kind {
    Kind::Popup { anchor } => Some(anchor.clone()),
    _ => None,
  };
  let modal = flags.modal;
  let pattern = bound.then(|| UiPattern {
    label: state.label.as_deref().map(Pattern::parse),
    tooltip: tooltip.as_deref().map(Pattern::parse),
  });
  let bind = bind.filter(|_| kind.takes("bind"));
  let is_list = matches!(kind, Kind::List(_));
  let list_count = state.list_count;

  let mut entity = world.spawn((
    UiNodeKind {
      kind,
      layout,
      flags,
      variant,
      action,
      nav: [None; 4],
      anchor_node: None,
    },
    UiInteraction::default(),
    UiTreeNode { key, instance },
    applied,
  ));
  if let Some(label) = state.label {
    entity.insert(UiLabel(label));
  }
  if let Some(image) = state.image {
    entity.insert(UiImage(image));
  }
  if let Some(checked) = state.checked {
    entity.insert(UiChecked(checked));
  }
  if let Some(value) = state.value {
    entity.insert(UiValue(value));
  }
  if let Some(index) = state.index {
    entity.insert(UiIndex(index));
  }
  if let Some(options) = state.options {
    entity.insert(UiOptions(options));
  }
  if let Some(text) = state.text {
    entity.insert(UiTextValue(text));
  }
  if let Some(slot) = state.slot {
    entity.insert(slot);
  }
  if let Some(reveal) = state.reveal {
    entity.insert(reveal);
  }
  if let Some(bind) = bind {
    entity.insert(UiBind(bind));
  }
  if let Some(pattern) = pattern.filter(|p| p.label.is_some() || p.tooltip.is_some()) {
    entity.insert(pattern);
  }
  if let Some(name) = name {
    entity.insert(UiName(name));
  }
  if let Some(tooltip) = tooltip {
    entity.insert(UiTooltip(tooltip));
  }
  if hidden {
    entity.insert(UiHidden);
  }
  if disabled {
    entity.insert(UiDisabled);
  }
  if let Some(parent) = parent {
    entity.insert(ChildOf(parent));
  }
  if let Some(source) = source {
    let generation = source.generation;
    entity.insert(UiSource { source, generation });
  }
  if is_list {
    entity.insert(UiList {
      count: list_count.unwrap_or(0),
      ..UiList::default()
    });
    match row_shape {
      Some(shape) => {
        entity.insert(UiListShape(shape));
      }
      None => list::warn_shapeless(),
    }
  }
  // After the engine's components, so a game component may replace one
  for insert in inserts {
    insert(&mut entity);
  }
  let id = entity.id();
  if is_list {
    world
      .get_resource_or_insert_with(UiNodes::default)
      .lists
      .push(id);
  }

  pending.push(Pending {
    entity: id,
    nav,
    popup,
    modal,
  });
  for (child, key) in children.into_iter().zip(child_keys) {
    spawn_node(world, Some(id), false, child, key, pending);
  }
  id
}

/// Resolve names, duplicates and confirms once a tree exists, or once an
/// update has brought it in line; each written only on a difference
pub(crate) fn finish(world: &mut World, top: Entity, pending: Vec<Pending>) {
  // A list's row resolves names within itself
  let root = scope_of(world, top).unwrap_or(top);
  for node in &pending {
    if let Some(name) = world.get::<UiName>(node.entity)
      && find_ui_in(world, root, &name.0) != Some(node.entity)
    {
      let name = &name.0;
      warn_once(format!("ui-name:twice:{name}"), || {
        format!("two UI nodes are named \"{name}\"; find_ui finds the first")
      });
    }

    let mut nav = [None; 4];
    for (i, name) in node.nav.iter().enumerate() {
      if let Some(name) = name {
        nav[i] = resolve(world, root, name, NAV_OPTIONS[i]);
      }
    }
    let anchor = node
      .popup
      .as_deref()
      .and_then(|name| resolve(world, root, name, "popup"));
    if let Some(mut kind) = world.get_mut::<UiNodeKind>(node.entity)
      && (kind.nav != nav || kind.anchor_node != anchor)
    {
      kind.nav = nav;
      kind.anchor_node = anchor;
    }
  }

  for node in &pending {
    set_back_answer(world, node.entity, node.modal);
  }
}

/// Bring the Back answer of each modal ancestor of `top` in line, after an
/// update of `top`'s subtree (which `finish` covers) changed what is below
/// them
pub(crate) fn answer_back_above(world: &mut World, top: Entity) {
  let mut at = top;
  while world.get::<UiRoot>(at).is_none() {
    let Some(parent) = world.get::<ChildOf>(at).map(ChildOf::parent) else {
      return;
    };
    let modal = world
      .get::<UiNodeKind>(parent)
      .is_some_and(|node| node.flags.modal);
    if modal {
      set_back_answer(world, parent, true);
    }
    at = parent;
  }
}

/// Point a modal node's `BackAnswer` at its first descendant that answers
/// Back, or take it away; written only on a difference
fn set_back_answer(world: &mut World, node: Entity, modal: bool) {
  let answer = modal
    .then(|| first_answering_descendant(world, node))
    .flatten();
  let held = world.get::<BackAnswer>(node).map(|answer| answer.0);
  if held != answer {
    match answer {
      Some(answer) => world.entity_mut(node).insert(BackAnswer(answer)),
      None => world.entity_mut(node).remove::<BackAnswer>(),
    };
  }
}

/// The node a name stands for in a root tree, warning once if there is none
fn resolve(world: &World, root: Entity, name: &str, option: &str) -> Option<Entity> {
  let found = find_ui_in(world, root, name);
  if found.is_none() {
    warn_once(format!("ui-name:{option}:{name}"), || {
      format!("no UI node named \"{name}\" for {option}")
    });
  }
  found
}

/// The first descendant, depth first, that answers Back
fn first_answering_descendant(world: &World, of: Entity) -> Option<Entity> {
  let kids = world.get::<Children>(of)?;
  for &child in kids.iter() {
    if world
      .get::<UiNodeKind>(child)
      .is_some_and(|node| node.flags.answers_back)
    {
      return Some(child);
    }
    if let Some(found) = first_answering_descendant(world, child) {
      return Some(found);
    }
  }
  None
}

/// The first node named `name` under `root` (included), depth first. The
/// rows of a list under it are their own scopes: they are searched only
/// from a row itself.
pub(crate) fn find_ui_in(world: &World, root: Entity, name: &str) -> Option<Entity> {
  if world.get::<UiName>(root).is_some_and(|node| node.0 == name) {
    return Some(root);
  }
  let kids = world.get::<Children>(root)?;
  kids
    .iter()
    .filter(|&&child| world.get::<UiListRow>(child).is_none())
    .find_map(|&child| find_ui_in(world, child, name))
}

/// The first node named `name` in any of `scene`'s trees, in spawn order
pub(crate) fn find_ui(world: &World, scene: u64, name: &str) -> Option<Entity> {
  let nodes = world.get_resource::<UiNodes>()?;
  nodes
    .roots
    .iter()
    .filter(|(owner, _)| *owner == scene)
    .find_map(|&(_, root)| find_ui_in(world, root, name))
}

/// The root of the tree `node` is in
pub(crate) fn root_of(world: &World, node: Entity) -> Option<Entity> {
  let mut at = node;
  loop {
    if world.get::<UiRoot>(at).is_some() {
      return Some(at);
    }
    at = world.get::<ChildOf>(at)?.parent();
  }
}

/// The scope `node` is in for its names and bindings: the nearest of it and
/// its ancestors that is a tree's root or a list's row
pub(crate) fn scope_of(world: &World, node: Entity) -> Option<Entity> {
  let mut at = node;
  loop {
    let entity = world.get_entity(at).ok()?;
    if entity.contains::<UiRoot>() || entity.contains::<UiListRow>() {
      return Some(at);
    }
    at = entity.get::<ChildOf>()?.parent();
  }
}

/// Despawn every tree `scene` owns
pub(crate) fn despawn_scene_roots(world: &mut World, scene: u64) {
  let Some(mut nodes) = world.get_resource_mut::<UiNodes>() else {
    return;
  };
  let mut owned = Vec::new();
  nodes.roots.retain(|&(owner, root)| {
    if owner == scene {
      owned.push(root);
    }
    owner != scene
  });
  for root in owned {
    if world.get_entity(root).is_ok() {
      world.despawn(root);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{ChildOf, Children, Component};
  use crate::core::testing::UiHarness;
  use crate::core::ui::test_util::warnings_containing as warnings;
  use crate::core::ui::{Anchor, UiNode};

  #[derive(Component, Debug, PartialEq)]
  struct SlotIndex(usize);

  fn children(world: &World, of: Entity) -> Vec<Entity> {
    world
      .get::<Children>(of)
      .map(|c| c.to_vec())
      .unwrap_or_default()
  }

  #[test]
  fn a_tree_spawns_an_entity_per_node_in_order() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::button("A"))
        .child(UiNode::row().child(UiNode::text("B")))
        .child(UiNode::checkbox("C", true)),
    );
    let w = h.world();
    let kids = children(w, root);
    assert_eq!(kids.len(), 3);
    assert_eq!(w.get::<UiLabel>(kids[0]), Some(&UiLabel("A".into())));
    assert_eq!(w.get::<ChildOf>(kids[0]).map(|c| c.parent()), Some(root));
    assert_eq!(
      w.get::<UiLabel>(children(w, kids[1])[0]),
      Some(&UiLabel("B".into()))
    );
    assert_eq!(w.get::<UiChecked>(kids[2]), Some(&UiChecked(true)));
    for node in [root, kids[0], kids[1], kids[2]] {
      assert!(w.get::<UiNodeKind>(node).is_some() && w.get::<UiInteraction>(node).is_some());
    }
    let ui_root = w.get::<UiRoot>(root).unwrap();
    assert_eq!((ui_root.anchor, ui_root.scene), (Anchor::TopLeft, 1));
    assert!(w.get::<UiRoot>(kids[0]).is_none());
  }

  #[test]
  fn markers_tooltips_and_inserts_land_on_their_node() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(
      UiNode::grid(2)
        .child(
          UiNode::slot()
            .tooltip("Sword")
            .insert(SlotIndex(3))
            .name("first"),
        )
        .child(UiNode::button("Hidden").hidden())
        .child(UiNode::button("Off").enabled(false)),
    );
    let w = h.world();
    let kids = children(w, root);
    assert_eq!(w.get::<SlotIndex>(kids[0]), Some(&SlotIndex(3)));
    assert_eq!(
      w.get::<UiTooltip>(kids[0]),
      Some(&UiTooltip("Sword".into()))
    );
    assert_eq!(w.get::<UiName>(kids[0]), Some(&UiName("first".into())));
    assert!(w.get::<UiHidden>(kids[1]).is_some());
    assert!(w.get::<UiDisabled>(kids[2]).is_some());
    assert_eq!(h.find_ui("first"), Some(kids[0]));
    assert_eq!(h.find_ui("nobody"), None);
  }

  #[test]
  fn find_ui_in_searches_one_tree() {
    let mut h = UiHarness::new(320.0, 240.0);
    let a = h.spawn_ui(UiNode::column().child(UiNode::button("A").name("ok")));
    let b = h.spawn_ui(UiNode::column().child(UiNode::button("B").name("ok")));
    let w = h.world();
    assert_eq!(find_ui_in(w, b, "ok"), Some(children(w, b)[0]));
    assert_eq!(
      h.find_ui("ok"),
      Some(children(w, a)[0]),
      "the first root first"
    );
  }

  #[test]
  fn neighbour_names_resolve_within_the_root() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::button("Play").nav_up("quit"))
        .child(UiNode::button("Quit").name("quit")),
    );
    let w = h.world();
    let kids = children(w, root);
    assert_eq!(w.get::<UiNodeKind>(kids[0]).unwrap().nav[0], Some(kids[1]));
  }

  #[test]
  fn unknown_neighbour_names_warn_once() {
    let mut h = UiHarness::new(320.0, 240.0);
    for _ in 0..2 {
      h.spawn_ui(UiNode::column().child(UiNode::button("Play").nav_down("nobody-4c-test")));
    }
    assert_eq!(warnings("nobody-4c-test"), 1);
  }

  #[test]
  fn a_name_used_twice_warns_once() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::button("A").name("twice-4c"))
        .child(UiNode::button("B").name("twice-4c")),
    );
    assert_eq!(warnings("two UI nodes are named \"twice-4c\""), 1);
    assert_eq!(
      h.find_ui("twice-4c"),
      Some(children(h.world(), root)[0]),
      "the first"
    );
  }

  #[test]
  fn an_option_the_kind_ignores_warns_once() {
    let mut h = UiHarness::new(320.0, 240.0);
    for _ in 0..2 {
      h.spawn_ui(UiNode::progress(0.5).password());
    }
    assert_eq!(
      warnings("UiNode::password does nothing on a progress node"),
      1
    );
  }

  #[test]
  fn spawn_ui_in_appends_and_resolves_names() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(UiNode::column().child(UiNode::button("Top").name("top")));
    let choices = UiNode::column().child(UiNode::button("Back up").nav_up("top"));
    let added = spawn_ui_in(h.world_mut(), root, choices).expect("a column takes children");
    let w = h.world();
    let kids = children(w, root);
    assert_eq!(kids.last(), Some(&added), "appended last");
    let button = children(w, added)[0];
    assert_eq!(w.get::<UiNodeKind>(button).unwrap().nav[0], Some(kids[0]));
    assert!(w.get::<UiRoot>(added).is_none());
    let leaf = kids[0];
    assert!(
      spawn_ui_in(h.world_mut(), leaf, UiNode::text("no")).is_none(),
      "a button holds no children"
    );
  }

  #[test]
  fn roots_keep_spawn_order_and_leave_with_their_scene() {
    let mut h = UiHarness::new(320.0, 240.0);
    let a = h.spawn_ui(UiNode::column());
    let b = h.spawn_ui(UiNode::column().child(UiNode::text("x")));
    let w = h.world();
    assert!(w.get::<UiRoot>(a).unwrap().seq < w.get::<UiRoot>(b).unwrap().seq);
    let kid = children(w, b)[0];
    despawn_scene_roots(h.world_mut(), 1);
    let w = h.world();
    assert!(w.get_entity(a).is_err() && w.get_entity(b).is_err() && w.get_entity(kid).is_err());
    assert!(w.resource::<UiNodes>().roots.is_empty());
  }

  #[test]
  fn a_borrowed_key_compares_as_written_out() {
    let nth = Key::Nth("button", 12);
    assert_eq!(nth.to_string(), "button#12");
    assert!(nth.is("button#12"));
    for other in [
      "button#012",
      "button#+12",
      "button#1",
      "buttons#12",
      "button#",
      "button12",
    ] {
      assert!(!nth.is(other), "{other}");
    }
    assert!(Key::Nth("text", 0).is("text#0"));
    assert!(Key::Given("ok").is("ok") && !Key::Given("ok").is("ok#0"));
  }

  #[test]
  fn siblings_keyed_the_same_warn_once() {
    let mut h = UiHarness::new(320.0, 240.0);
    for _ in 0..2 {
      h.spawn_ui(
        UiNode::column()
          .child(UiNode::button("A").key("twice-4d"))
          .child(UiNode::button("B").key("twice-4d"))
          .child(UiNode::button("C").key("twice-4d")),
      );
    }
    assert_eq!(warnings("keyed \"twice-4d\""), 1);
  }

  #[test]
  fn keys_come_from_key_then_name_then_kind_and_index() {
    let children = vec![
      UiNode::button("A"),
      UiNode::button("B").name("b"),
      UiNode::button("C"),
      UiNode::text("T"),
      UiNode::button("D").key("d"),
    ];
    assert_eq!(
      child_keys(&children),
      ["button#0", "b", "button#1", "text#0", "d"]
    );
  }
}
