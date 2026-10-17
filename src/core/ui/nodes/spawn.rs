//! Spawning a node tree as entities, finding nodes by name, and despawning a scene's trees

use glam::Vec2;

use crate::core::ecs::kernel::{ChildOf, Children, Entity, World};
use crate::core::ui::Anchor;
use crate::core::ui::theme::warn_once;

use super::components::{
  BackAnswer, UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiInteraction, UiLabel, UiName,
  UiNodes, UiOptions, UiRoot, UiTextValue, UiTooltip, UiValue,
};
use super::node::{Kind, UiNode, UiNodeKind};

const NAV_OPTIONS: [&str; 4] = ["nav_up", "nav_down", "nav_left", "nav_right"];

/// What a spawned node still needs once its whole tree exists
struct Pending {
  entity: Entity,
  nav: [Option<String>; 4],
  popup: Option<String>,
  modal: bool,
}

/// Spawn `tree` as a root owned by `scene`
pub(crate) fn spawn_ui(world: &mut World, scene: u64, tree: UiNode) -> Entity {
  let anchor = tree.layout.anchor.unwrap_or(Anchor::TopLeft);
  let offset = tree.layout.offset.unwrap_or(Vec2::ZERO);
  let (at, embedded) = (tree.layout.at, tree.embedded);
  let mut pending = Vec::new();
  let top = spawn_node(world, None, true, tree, &mut pending);

  let mut nodes = world.get_resource_or_insert_with(UiNodes::default);
  let seq = nodes.seq;
  nodes.seq += 1;
  nodes.roots.push((scene, top));
  world.entity_mut(top).insert(UiRoot {
    anchor,
    offset,
    at,
    scene,
    seq,
    embedded,
  });

  finish(world, top, pending);
  top
}

/// Spawn `tree` under `parent`, after its last child; `None` if `parent` is
/// not a container node
pub(crate) fn spawn_ui_in(world: &mut World, parent: Entity, tree: UiNode) -> Option<Entity> {
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
  let mut pending = Vec::new();
  let top = spawn_node(world, Some(parent), false, tree, &mut pending);
  finish(world, top, pending);
  Some(top)
}

/// Spawn one node and, depth first, its children
fn spawn_node(
  world: &mut World,
  parent: Option<Entity>,
  is_root: bool,
  node: UiNode,
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

  let popup = match &kind {
    Kind::Popup { anchor } => Some(anchor.clone()),
    _ => None,
  };
  let modal = flags.modal;

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
  // After the engine's components, so a game component may replace one
  for insert in inserts {
    insert(&mut entity);
  }
  let id = entity.id();

  pending.push(Pending {
    entity: id,
    nav,
    popup,
    modal,
  });
  for child in children {
    spawn_node(world, Some(id), false, child, pending);
  }
  id
}

/// Resolve names, duplicates and confirms once a tree exists
fn finish(world: &mut World, top: Entity, pending: Vec<Pending>) {
  let root = root_of(world, top).unwrap_or(top);
  for node in &pending {
    if let Some(name) = world.get::<UiName>(node.entity).map(|name| name.0.clone())
      && find_ui_in(world, root, &name) != Some(node.entity)
    {
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
    if let Some(mut kind) = world.get_mut::<UiNodeKind>(node.entity) {
      kind.nav = nav;
      kind.anchor_node = anchor;
    }
  }

  for node in pending.iter().filter(|node| node.modal) {
    if let Some(answer) = first_answering_descendant(world, node.entity) {
      world.entity_mut(node.entity).insert(BackAnswer(answer));
    }
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

/// The first node named `name` under `root` (included), depth first
pub(crate) fn find_ui_in(world: &World, root: Entity, name: &str) -> Option<Entity> {
  if world.get::<UiName>(root).is_some_and(|node| node.0 == name) {
    return Some(root);
  }
  let kids = world.get::<Children>(root)?;
  kids
    .iter()
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

/// Despawn every tree `scene` owns
#[allow(dead_code)] // called once scenes own their trees
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
  use crate::core::ui::{Anchor, UiNode};

  #[derive(Component, Debug, PartialEq)]
  struct SlotIndex(usize);

  fn children(world: &World, of: Entity) -> Vec<Entity> {
    world
      .get::<Children>(of)
      .map(|c| c.to_vec())
      .unwrap_or_default()
  }

  fn warnings(needle: &str) -> usize {
    crate::core::dev_log::get_entries()
      .iter()
      .filter(|w| w.message.contains(needle))
      .count()
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
}
