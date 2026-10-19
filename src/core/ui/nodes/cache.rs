//! The node cache: each node's last build, and which nodes changed since
//!
//! A node's record holds the tape of what its builder pushed. The dirty set
//! holds the nodes that changed since their records were made (a tracked
//! component changed or was removed, a child came or went), each with its
//! ancestors up to its root. `refresh_dirty` brings it up to date from
//! change ticks and removal messages since the last check. The cache keeps
//! its own last tick: the engine clears its trackers in `Last`, after the UI
//! pass, so the world's own would miss the previous frame's `update`.

use crate::core::ecs::kernel::{
  Changed, CheckChangeTicks, ChildOf, Children, ComponentId, Entity, EntityHashMap, EntityHashSet,
  MessageCursor, Mut, Or, QueryState, RemovedComponentEntity, Tick, With, World, WorldId,
};
use crate::core::text::TextReveal;
use crate::core::ui::UiId;
use crate::core::ui::env::UiEnv;
use crate::core::ui::tape::Tape;

use super::bindings::{UiBind, UiPattern};
use super::components::{
  UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiLabel, UiNodes, UiOptions, UiRoot, UiSlot,
  UiTextValue, UiTooltip, UiValue,
};
use super::list::UiListRow;
use super::node::UiNodeKind;

/// The components a node's build reads, from one list: what `draw_node`
/// reads of every node (`NodeRead`), what each kind's draw reads, the list
/// row's index, the binding patterns and the hierarchy. `Changes` is a
/// change to any of them, in two halves (bevy's `Or` takes at most 15);
/// `REMOVALS` registers each one whose removal is read. `ChildOf` is only
/// changed: its removal changes the old parent's `Children`, which is read.
macro_rules! tracked {
  ([$($a:ty),* $(,)?], [$($b:ty),* $(,)?], changed only: $c:ty) => {
    type Changes = Or<(Or<($(Changed<$a>,)*)>, Or<($(Changed<$b>,)* Changed<$c>)>)>;
    const REMOVALS: &[fn(&mut World) -> ComponentId] = &[
      $(World::register_component::<$a>,)*
      $(World::register_component::<$b>,)*
    ];
  };
}

tracked!(
  [UiNodeKind, UiRoot, UiLabel, UiValue, UiChecked, UiIndex, UiOptions, UiTextValue, UiSlot, UiImage],
  [UiTooltip, UiHidden, UiDisabled, UiBind, UiPattern, TextReveal, UiListRow, Children],
  changed only: ChildOf
);

/// What the node walk keeps between frames: each node's last build, and
/// which nodes changed since
#[allow(dead_code)] // enabled, records and stats until the walk reads them
pub(crate) struct UiCache {
  /// Whether the walk replays clean nodes
  pub(crate) enabled: bool,
  /// Each node's last build
  pub(crate) records: EntityHashMap<NodeRecord>,
  /// The nodes that changed since their records were made, and their
  /// ancestors, until rebuilt
  pub(crate) dirty: EntityHashSet,
  /// The change tick the last check saw up to
  pub(crate) checked: Option<Tick>,
  /// What the last pass built and replayed
  pub(crate) stats: CacheStats,
  /// The check's query and removal cursors, made for one world
  check: Option<Box<Check>>,
}

impl Default for UiCache {
  fn default() -> UiCache {
    UiCache {
      enabled: true,
      records: EntityHashMap::default(),
      dirty: EntityHashSet::default(),
      checked: None,
      stats: CacheStats::default(),
      check: None,
    }
  }
}

/// A node's last build: what its builder pushed and what it was built under
#[derive(Clone)]
#[allow(dead_code)] // until the walk keeps records
pub(crate) struct NodeRecord {
  /// What the builder pushed
  pub(crate) tape: Tape,
  /// The scene that drew it
  pub(crate) scene: u64,
  /// The environment it was built in
  pub(crate) env: UiEnv,
  /// Whether its scene was on top
  pub(crate) top: bool,
  /// The memory frame it was built in
  pub(crate) frame: u64,
  /// The node's response id: what a replay returns, with last frame's rect
  pub(crate) id: UiId,
  /// A modal or popup container: the walk's Back target
  pub(crate) dialog: bool,
}

/// What the walk did with the nodes of its last pass
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(dead_code)] // until the walk counts them
pub(crate) struct CacheStats {
  /// Nodes built
  pub(crate) built: usize,
  /// Nodes replayed
  pub(crate) replayed: usize,
  /// The nodes built, in walk order
  pub(crate) rebuilt: Vec<Entity>,
}

/// The dirty check's query and removal cursors, made for one world
struct Check {
  world: WorldId,
  /// The nodes whose tracked components changed since the check's last tick
  changes: QueryState<Entity, (With<UiNodeKind>, Changes)>,
  removals: Vec<Removals>,
}

/// One tracked component's removal messages, read from where the last
/// check stopped. They last two tracker clears, so a cursor read every pass
/// misses none.
struct Removals {
  component: ComponentId,
  cursor: MessageCursor<RemovedComponentEntity>,
}

impl Check {
  fn new(world: &mut World) -> Check {
    let changes = QueryState::new(world);
    let removals = REMOVALS
      .iter()
      .map(|register| Removals {
        component: register(world),
        cursor: MessageCursor::default(),
      })
      .collect();
    Check {
      world: world.id(),
      changes,
      removals,
    }
  }

  /// Mark the entities each tracked component was removed from since the
  /// last read, or with no `dirty` only move past them. Whether any were
  /// missed: left unread for two tracker clears.
  fn read_removals(&mut self, world: &World, mut dirty: Option<&mut EntityHashSet>) -> bool {
    let mut missed = false;
    for removals in &mut self.removals {
      let Some(messages) = world.removed_components().get(removals.component) else {
        continue;
      };
      match dirty.as_deref_mut() {
        Some(dirty) => {
          missed |= removals.cursor.missed_messages(messages) > 0;
          for removed in removals.cursor.read(messages) {
            mark(dirty, world, Entity::from(removed.clone()));
          }
        }
        None => removals.cursor.clear(messages),
      }
    }
    missed
  }
}

impl UiCache {
  /// Bring `dirty` up to date with what changed in `world` since the last
  /// check
  fn refresh(&mut self, world: &mut World) {
    let id = world.id();
    if self.check.as_ref().is_none_or(|check| check.world != id) {
      self.check = Some(Box::new(Check::new(world)));
      self.checked = None;
    }
    let Some(check) = self.check.as_deref_mut() else {
      return;
    };
    match self.checked {
      // The first check marks nothing: a node without a record is built
      // anyway
      None => {
        check.read_removals(world, None);
      }
      Some(last) => {
        let dirty = &mut self.dirty;
        world.last_change_tick_scope(last, |world| {
          for node in check.changes.iter(world) {
            mark(dirty, world, node);
          }
        });
        if check.read_removals(world, Some(dirty)) {
          // Which nodes the missed removals touched is unknown: every node
          // is built again
          self.records.clear();
        }
      }
    }
    // Writes made after this check (by the scene's `ui`, or before the next
    // scene's walk) would share its tick, since nothing in a UI pass advances
    // the world's: move past it, so they are newer than `checked`
    self.checked = Some(world.increment_change_tick());
  }
}

/// Bring the dirty set up to date from the tracked components' change ticks
/// and removals since the last check. A world without node trees is left
/// as it is.
#[allow(dead_code)] // until the walk checks before it draws
pub(crate) fn refresh_dirty(world: &mut World) {
  world.try_resource_scope(|world, mut nodes: Mut<UiNodes>| nodes.cache.refresh(world));
}

/// Mark `node` dirty, with its ancestors up to its root
#[allow(dead_code)] // until the walk marks what a built node writes
pub(crate) fn mark_dirty(world: &mut World, node: Entity) {
  let Some(mut nodes) = world.get_resource_mut::<UiNodes>() else {
    return;
  };
  let mut dirty = std::mem::take(&mut nodes.cache.dirty);
  mark(&mut dirty, world, node);
  if let Some(mut nodes) = world.get_resource_mut::<UiNodes>() {
    nodes.cache.dirty = dirty;
  }
}

/// Insert `node` and walk `ChildOf` upward inserting each ancestor, up to a
/// root or a node already dirty (its ancestors are). An entity that is not
/// a node (its `UiNodeKind` was removed) marks its parent, whose draw read
/// it. A despawned one marks nothing (its parent's `Children` changed) and
/// leaves the set: it is never built again.
fn mark(dirty: &mut EntityHashSet, world: &World, node: Entity) {
  let mut at = node;
  let mut first = true;
  loop {
    let Ok(entity) = world.get_entity(at) else {
      if first {
        dirty.remove(&at);
      }
      return;
    };
    if entity.contains::<UiNodeKind>() {
      if !dirty.insert(at) || entity.contains::<UiRoot>() {
        return;
      }
    } else if !first {
      return;
    }
    first = false;
    let Some(parent) = entity.get::<ChildOf>() else {
      return;
    };
    at = parent.parent();
  }
}

/// Clamp the tick the cache keeps after the world checked its own (bevy
/// clamps only the ticks it holds)
pub(crate) fn check_ticks(cache: &mut UiCache, check: CheckChangeTicks) {
  if let Some(tick) = cache.checked.as_mut() {
    tick.check_tick(check);
  }
}

/// [`check_ticks`] for the cache of the trees in `world`, if it has any:
/// the engine calls it where it clamps its schedules' ticks
pub(crate) fn check_cache_ticks(world: &mut World, check: CheckChangeTicks) {
  if let Some(mut nodes) = world.get_resource_mut::<UiNodes>() {
    check_ticks(&mut nodes.cache, check);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn tree(h: &mut UiHarness) -> (Entity, Entity, Entity) {
    let root = h.spawn_ui(
      UiNode::column().name("root").child(
        UiNode::row()
          .name("row")
          .child(UiNode::button("Go").name("go")),
      ),
    );
    h.frame(|_| ());
    let row = h.find_ui("row").unwrap();
    let go = h.find_ui("go").unwrap();
    (root, row, go)
  }

  fn dirty(h: &mut UiHarness) -> EntityHashSet {
    refresh_dirty(h.world_mut());
    h.world()
      .get_resource::<UiNodes>()
      .map(|n| n.cache.dirty.clone())
      .unwrap_or_default()
  }

  fn clear(h: &mut UiHarness) {
    refresh_dirty(h.world_mut());
    if let Some(mut n) = h.world_mut().get_resource_mut::<UiNodes>() {
      n.cache.dirty.clear();
    }
  }

  #[test]
  fn a_changed_label_marks_its_node_and_ancestors() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (root, row, go) = tree(&mut h);
    clear(&mut h);
    h.world_mut().get_mut::<UiLabel>(go).unwrap().0 = "Stop".into();
    let d = dirty(&mut h);
    assert!(d.contains(&go) && d.contains(&row) && d.contains(&root));
  }

  #[test]
  fn nothing_changed_marks_nothing() {
    let mut h = UiHarness::new(640.0, 480.0);
    tree(&mut h);
    clear(&mut h);
    assert!(dirty(&mut h).is_empty());
  }

  #[test]
  fn a_change_made_in_the_previous_update_is_seen_after_trackers_clear() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, _, go) = tree(&mut h);
    clear(&mut h);
    h.world_mut().get_mut::<UiLabel>(go).unwrap().0 = "Later".into();
    h.world_mut().clear_trackers(); // the engine clears in Last, after the UI pass
    assert!(dirty(&mut h).contains(&go));
  }

  #[test]
  fn a_removed_component_marks_its_node() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, _, go) = tree(&mut h);
    h.world_mut().entity_mut(go).insert(UiDisabled);
    clear(&mut h);
    h.world_mut().entity_mut(go).remove::<UiDisabled>();
    assert!(dirty(&mut h).contains(&go));
  }

  #[test]
  fn a_despawned_child_marks_its_parent() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, row, go) = tree(&mut h);
    clear(&mut h);
    h.world_mut().despawn(go);
    assert!(dirty(&mut h).contains(&row));
  }

  #[test]
  fn a_despawned_node_leaves_the_dirty_set() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, row, go) = tree(&mut h);
    clear(&mut h);
    h.world_mut().get_mut::<UiLabel>(go).unwrap().0 = "Stop".into();
    assert!(dirty(&mut h).contains(&go));
    h.world_mut().despawn(go);
    let d = dirty(&mut h);
    assert!(!d.contains(&go), "never built again");
    assert!(d.contains(&row));
  }

  #[test]
  fn removals_missed_for_two_tracker_clears_drop_every_record() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, _, go) = tree(&mut h);
    h.world_mut().entity_mut(go).insert(UiDisabled);
    clear(&mut h);
    let record = NodeRecord {
      tape: Tape::default(),
      scene: 1,
      env: UiEnv::default(),
      top: true,
      frame: 1,
      id: UiId::NONE,
      dialog: false,
    };
    let records = |h: &mut UiHarness| h.world_mut().resource_mut::<UiNodes>().cache.records.len();
    h.world_mut()
      .resource_mut::<UiNodes>()
      .cache
      .records
      .insert(go, record);
    h.world_mut().entity_mut(go).remove::<UiDisabled>();
    h.world_mut().clear_trackers();
    refresh_dirty(h.world_mut());
    assert_eq!(records(&mut h), 1, "read within two clears: nothing missed");
    h.world_mut().entity_mut(go).insert(UiDisabled);
    h.world_mut().entity_mut(go).remove::<UiDisabled>();
    h.world_mut().clear_trackers();
    h.world_mut().clear_trackers();
    refresh_dirty(h.world_mut());
    assert_eq!(records(&mut h), 0, "a missed removal rebuilds everything");
  }

  #[test]
  fn a_world_without_ui_is_left_alone() {
    let mut world = World::new();
    let before = world.entities().len();
    refresh_dirty(&mut world);
    assert_eq!(world.entities().len(), before);
    assert!(world.get_resource::<UiNodes>().is_none());
  }

  #[test]
  fn the_first_check_after_records_exist_marks_nothing_when_nothing_changed() {
    let mut h = UiHarness::new(640.0, 480.0);
    let root = h.spawn_ui(
      UiNode::column()
        .name("root")
        .child(UiNode::text("Title").name("title"))
        .child(UiNode::slider(0.0..=1.0, 0.5).name("volume"))
        .child(UiNode::checkbox("Mute", false).name("mute")),
    );
    // The first check: every node is new, and every node without a record is
    // built anyway
    assert!(dirty(&mut h).is_empty());
    // The walk builds every node; the records it would store, by hand
    h.frame(|_| ());
    let mut nodes = vec![root];
    for name in ["title", "volume", "mute"] {
      nodes.push(h.find_ui(name).unwrap());
    }
    let env = h.frame(|ui| ui.env());
    let mut ui_nodes = h.world_mut().resource_mut::<UiNodes>();
    for &node in &nodes {
      let record = NodeRecord {
        tape: Tape::default(),
        scene: 1,
        env,
        top: true,
        frame: 1,
        id: UiId::NONE,
        dialog: false,
      };
      ui_nodes.cache.records.insert(node, record);
    }
    // Neither the spawn nor the frames that built the tree are changes
    assert!(dirty(&mut h).is_empty());
  }

  #[test]
  fn a_reparented_node_marks_its_old_and_new_parents() {
    let mut h = UiHarness::new(640.0, 480.0);
    let root = h.spawn_ui(
      UiNode::column()
        .name("root")
        .child(
          UiNode::column()
            .name("a")
            .child(UiNode::button("X").name("x"))
            .child(UiNode::button("Y").name("y")),
        )
        .child(
          UiNode::column()
            .name("b")
            .child(UiNode::button("Z").name("z")),
        ),
    );
    h.frame(|_| ());
    let [a, b, x, y, z] = ["a", "b", "x", "y", "z"].map(|name| h.find_ui(name).unwrap());
    clear(&mut h);
    h.world_mut().entity_mut(x).insert(ChildOf(b));
    let d = dirty(&mut h);
    assert!(d.contains(&x), "its id is now under another parent");
    assert!(d.contains(&a), "the old parent lost a child");
    assert!(d.contains(&b), "the new parent gained one");
    assert!(d.contains(&root));
    assert!(
      !d.contains(&y) && !d.contains(&z),
      "their siblings are unchanged"
    );
  }

  #[test]
  fn a_mark_stops_at_a_node_already_dirty() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (root, row, go) = tree(&mut h);
    clear(&mut h);
    // Marked by hand without its ancestors: a mark below it stops there
    h.world_mut()
      .resource_mut::<UiNodes>()
      .cache
      .dirty
      .insert(row);
    mark_dirty(h.world_mut(), go);
    let d = &h.world().resource::<UiNodes>().cache.dirty;
    assert!(d.contains(&go) && d.contains(&row));
    assert!(!d.contains(&root));
  }

  #[test]
  fn the_kept_tick_is_clamped_with_the_worlds() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, _, go) = tree(&mut h);
    clear(&mut h);
    // The world checks its ticks only after bevy's threshold: about a second
    // in debug builds
    for _ in 0..520_000_000u32 {
      h.world_mut().increment_change_tick();
    }
    let present = h.world_mut().change_tick();
    // A kept tick just older than bevy's maximum change age
    let stale = Tick::new(present.get().wrapping_sub(Tick::MAX.get() + 1_000));
    h.world_mut().resource_mut::<UiNodes>().cache.checked = Some(stale);
    assert!(h.check_change_ticks(), "the world checked its ticks");
    assert_eq!(
      h.world().resource::<UiNodes>().cache.checked,
      Some(Tick::new(present.get().wrapping_sub(Tick::MAX.get()))),
      "clamped to the oldest age bevy compares"
    );
    h.world_mut().get_mut::<UiLabel>(go).unwrap().0 = "After".into();
    assert!(dirty(&mut h).contains(&go));
  }
}
