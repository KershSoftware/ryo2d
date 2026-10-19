//! The node cache: each node's last build, and which nodes changed since
//!
//! A node's record holds the tape of what its builder pushed. The dirty set
//! holds the nodes that changed since their records were made (a tracked
//! component changed or was removed, a child came or went), each with its
//! ancestors up to its root. `refresh_dirty` brings it up to date from
//! change ticks and removal messages since the last check. The cache keeps
//! its own last tick: the engine clears its trackers in `Last`, after the UI
//! pass, so the world's own would miss the previous frame's `update`.
//!
//! The rest of what makes a node rebuild is marked by `mark_live` before
//! each walk: the nodes holding an interaction role this frame or the last,
//! and the live ones (scroll areas and lists, an unfinished typewriter, the
//! node a focus request waits for, and the nodes that read input or time
//! without holding a role while that moves). `clean` then adds what a
//! record itself says: it must be replayable, built in this environment and
//! top flag, and every read of its subtree must still hold.

use crate::core::ecs::kernel::{
  Allow, Changed, CheckChangeTicks, ChildOf, Children, ComponentId, Disabled, Entity,
  EntityHashMap, EntityHashSet, MessageCursor, Mut, Or, QueryState, RemovedComponentEntity, Tick,
  With, World, WorldId,
};
use crate::core::hash::FastMap;
use crate::core::text::TextReveal;
use crate::core::ui::env::UiEnv;
use crate::core::ui::memory::Roles;
use crate::core::ui::tape::{Read, Tape};
use crate::core::ui::widgets::rebind::RebindState;
use crate::core::ui::{UiContext, UiId};

use super::bindings::{UiBind, UiPattern};
use super::components::{
  UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiLabel, UiNodes, UiOptions, UiRoot, UiSlot,
  UiTextValue, UiTooltip, UiValue,
};
use super::draw::recorded_id;
use super::list::UiListRow;
use super::node::{Kind, UiNodeKind};

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
  /// The node each id a stored tape pushed belongs to: who holds a role
  pub(crate) by_id: FastMap<UiId, Entity>,
  /// The roles of the frame marked before this one: their holders draw
  /// their exit
  pub(crate) last_roles: Roles,
  /// The memory frame marked last, and its roles as last marked: they become
  /// `last_roles` when a later frame is marked
  roles: (u64, Roles),
  /// The nodes built every frame: scroll areas and lists (`note_live`)
  pub(crate) live: EntityHashSet,
  /// The tabs nodes, built while a tab switch is pressed
  tabs: EntityHashSet,
  /// The drop-target nodes, built while a drag is held
  targets: EntityHashSet,
  /// The choice lists, built while a number key is pressed
  choices: EntityHashSet,
  /// The rebinding rows, built while capturing or showing a note
  rebinds: EntityHashSet,
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
      by_id: FastMap::default(),
      last_roles: Roles::default(),
      roles: (0, Roles::default()),
      live: EntityHashSet::default(),
      tabs: EntityHashSet::default(),
      targets: EntityHashSet::default(),
      choices: EntityHashSet::default(),
      rebinds: EntityHashSet::default(),
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

/// Every node with a typewriter, `Disabled` ones too
type Reveals = QueryState<(Entity, &'static TextReveal), (With<UiNodeKind>, Allow<Disabled>)>;

/// The dirty check's queries and removal cursors, made for one world
struct Check {
  world: WorldId,
  /// The nodes whose tracked components changed since the check's last
  /// tick, `Disabled` ones too: the walk draws them
  changes: QueryState<Entity, (With<UiNodeKind>, Allow<Disabled>, Changes)>,
  /// The nodes with a typewriter: `mark_live` builds the unfinished ones
  reveals: Reveals,
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
    let reveals = QueryState::new(world);
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
      reveals,
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
  /// Make the check for `world` if it has none. A check made for another
  /// world starts the cache as new: nothing kept from there holds here.
  fn check_for(&mut self, world: &mut World) {
    match &self.check {
      Some(check) if check.world == world.id() => return,
      Some(_) => {
        self.records.clear();
        self.dirty.clear();
        self.by_id.clear();
        self.last_roles = Roles::default();
        self.roles = (0, Roles::default());
        self.live.clear();
        self.tabs.clear();
        self.targets.clear();
        self.choices.clear();
        self.rebinds.clear();
      }
      None => {}
    }
    self.check = Some(Box::new(Check::new(world)));
    self.checked = None;
  }

  /// Bring `dirty` up to date with what changed in `world` since the last
  /// check
  fn refresh(&mut self, world: &mut World) {
    self.check_for(world);
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

/// Mark what must be built in `scene`'s walk whatever changed, of the
/// nodes with a record of `scene` (one without is built anyway when drawn):
/// - the nodes holding an interaction role this frame or the last (found
///   through `by_id`);
/// - the live ones: scroll areas and lists, and unfinished typewriters;
/// - `pending`, the node a focus request waits for;
/// - in the top scene, the tabs nodes while a tab switch is pressed, and the
///   choice lists while a number key is;
/// - the drop targets while a drag is held;
/// - the rebinding rows capturing a press or showing a swap's note.
///
/// `ui` is recording `scene`. It costs the roles and the members of the
/// live sets, never a walk of every node.
#[allow(dead_code)] // until the walk marks before it draws
pub(crate) fn mark_live(
  world: &mut World,
  ui: &UiContext<'_>,
  scene: u64,
  pending: Option<Entity>,
) {
  world.try_resource_scope(|world, mut nodes: Mut<UiNodes>| {
    nodes.cache.mark_live(world, ui, scene, pending);
  });
}

impl UiCache {
  fn mark_live(
    &mut self,
    world: &mut World,
    ui: &UiContext<'_>,
    scene: u64,
    pending: Option<Entity>,
  ) {
    // First: a check made for another world starts the cache as new
    self.mark_reveals(world, scene);
    let interact = &ui.memory.interact;
    let now = interact.roles();
    let frame = ui.memory.frame();
    // A second pass in the same frame (another scene, a rerun) keeps last
    // frame's roles
    if self.roles.0 != frame {
      self.last_roles = self.roles.1;
    }
    self.roles = (frame, now);
    let world = &*world;
    let (dirty, records) = (&mut self.dirty, &self.records);
    let roles = now.iter().chain(self.last_roles.iter());
    let holders = roles.filter_map(|id| self.by_id.get(&id).copied());
    for node in holders.chain(pending) {
      if recorded_in(records, node, scene) {
        mark(dirty, world, node);
      }
    }
    let always = |_| true;
    mark_each(&mut self.live, dirty, records, world, scene, always);
    let top = ui.scene().1;
    if top && interact.tab_step != 0 {
      mark_each(&mut self.tabs, dirty, records, world, scene, always);
    }
    if top && ui.choice_key_pressed() {
      mark_each(&mut self.choices, dirty, records, world, scene, always);
    }
    if interact.drag.is_some() {
      mark_each(&mut self.targets, dirty, records, world, scene, always);
    }
    let busy = |node| {
      ui.memory
        .peek_state::<RebindState>(recorded_id(world, node))
        .is_some_and(|state| state.capturing || state.note.is_some())
    };
    mark_each(&mut self.rebinds, dirty, records, world, scene, busy);
  }

  /// Mark each typewriter in `scene` that an advance would still move
  fn mark_reveals(&mut self, world: &mut World, scene: u64) {
    self.check_for(world);
    let Some(check) = self.check.as_deref_mut() else {
      return;
    };
    let world = &*world;
    let (dirty, records) = (&mut self.dirty, &self.records);
    for (node, reveal) in check.reveals.iter(world) {
      if !reveal.idle() && recorded_in(records, node, scene) {
        mark(dirty, world, node);
      }
    }
  }
}

/// Whether `node` has a record of `scene`. One without is built anyway when
/// it is drawn; marking it would only dirty its ancestors (a hidden
/// branch's would rebuild the path above it every frame).
fn recorded_in(records: &EntityHashMap<NodeRecord>, node: Entity, scene: u64) -> bool {
  records
    .get(&node)
    .is_some_and(|record| record.scene == scene)
}

/// Mark each node of `set` with a record of `scene` that is live `when`;
/// despawned ones leave the set
fn mark_each(
  set: &mut EntityHashSet,
  dirty: &mut EntityHashSet,
  records: &EntityHashMap<NodeRecord>,
  world: &World,
  scene: u64,
  when: impl Fn(Entity) -> bool,
) {
  set.retain(|&node| {
    if world.get_entity(node).is_err() {
      return false;
    }
    if recorded_in(records, node, scene) && when(node) {
      mark(dirty, world, node);
    }
    true
  });
}

/// Keep `node`'s place in the live sets by its `kind`, when its record is
/// stored: a scroll area or a list is built every frame; a tabs node, a
/// choice list, a drop target and a rebinding row are built while what they
/// read without holding a role moves
#[allow(dead_code)] // until the walk stores records
pub(crate) fn note_live(cache: &mut UiCache, node: Entity, kind: &UiNodeKind) {
  let live = matches!(kind.kind, Kind::Scroll(_) | Kind::List(_));
  member(&mut cache.live, node, live);
  member(&mut cache.tabs, node, matches!(kind.kind, Kind::Tabs));
  let choices = matches!(kind.kind, Kind::ChoiceList { .. });
  member(&mut cache.choices, node, choices);
  member(&mut cache.targets, node, kind.flags.drop_target);
  member(
    &mut cache.rebinds,
    node,
    matches!(kind.kind, Kind::Rebind(_)),
  );
}

fn member(set: &mut EntityHashSet, node: Entity, is: bool) {
  if is {
    set.insert(node);
  } else {
    set.remove(&node);
  }
}

/// Whether the walk may replay `node`'s record instead of building it:
/// caching is on, it has a record, its tape is replayable, it was built in
/// this scene, environment and top flag, what it and its subtree read of
/// other nodes is unchanged (a tape carries its descendants' reads), and it
/// is not dirty
#[allow(dead_code)] // until the walk replays
pub(crate) fn clean(world: &World, ui: &UiContext<'_>, node: Entity) -> bool {
  let Some(nodes) = world.get_resource::<UiNodes>() else {
    return false;
  };
  let cache = &nodes.cache;
  if !cache.enabled || cache.dirty.contains(&node) {
    return false;
  }
  let Some(record) = cache.records.get(&node) else {
    return false;
  };
  let (scene, top) = ui.scene();
  record.tape.replayable
    && record.scene == scene
    && record.top == top
    && record.env == ui.env()
    && record.tape.reads.iter().all(|read| holds(world, ui, read))
}

/// Whether what a build read is still so: a node's id, or where a widget is
fn holds(world: &World, ui: &UiContext<'_>, read: &Read) -> bool {
  match *read {
    Read::Id { node, id } => recorded_id(world, node) == id,
    Read::Rect { id, rect } => ui.anchor_rect(id) == rect,
  }
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
  fn a_disabled_entity_is_still_checked() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (root, row, go) = tree(&mut h);
    h.world_mut()
      .entity_mut(go)
      .insert((Disabled, UiTooltip("Tip".into())));
    h.world_mut().entity_mut(row).insert(Disabled);
    clear(&mut h);
    // The walk draws a node bevy's `Disabled` hides from default queries
    h.world_mut().get_mut::<UiLabel>(go).unwrap().0 = "Stop".into();
    let d = dirty(&mut h);
    assert!(d.contains(&go) && d.contains(&row) && d.contains(&root));
    clear(&mut h);
    h.world_mut().entity_mut(go).remove::<UiTooltip>();
    let d = dirty(&mut h);
    assert!(d.contains(&go) && d.contains(&row) && d.contains(&root));
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
  fn another_world_starts_the_cache_afresh() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, _, go) = tree(&mut h);
    clear(&mut h);
    let mut nodes = h.world_mut().remove_resource::<UiNodes>().unwrap();
    nodes.cache.dirty.insert(go);
    let record = NodeRecord {
      tape: Tape::default(),
      scene: 1,
      env: UiEnv::default(),
      top: true,
      frame: 1,
      id: UiId::NONE,
      dialog: false,
    };
    nodes.cache.records.insert(go, record);
    let mut other = World::new();
    other.insert_resource(nodes);
    refresh_dirty(&mut other);
    let cache = &other.resource::<UiNodes>().cache;
    assert!(cache.records.is_empty() && cache.dirty.is_empty());
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

#[cfg(all(test, feature = "default-fonts"))]
mod live_tests {
  use super::*;
  use crate::core::input::{Binding, InputDevice};
  use crate::core::testing::UiHarness;
  use crate::core::ui::id::UiId;
  use crate::core::ui::memory::Interaction;
  use crate::core::ui::nodes::*;
  use crate::core::ui::tape::Read;
  use crate::core::ui::test_util::{UiDriver, prompts_with};
  use crate::prelude::*;
  use winit::keyboard::KeyCode;

  #[test]
  fn roles_hold_every_id_with_a_part_this_frame() {
    let i = Interaction {
      hovered: UiId(11),
      focused: UiId(12),
      clicked: UiId(13),
      drop_hover: UiId(14),
      ..Interaction::default()
    };
    let roles = i.roles();
    for id in [11, 12, 13, 14] {
      assert!(roles.contains(UiId(id)), "{id}");
    }
    assert!(!roles.contains(UiId(99)));
  }

  fn cache_mut(h: &mut UiHarness) -> Mut<'_, UiNodes> {
    h.world_mut().resource_mut::<UiNodes>()
  }

  fn warm(h: &mut UiHarness) {
    for _ in 0..3 {
      h.frame(|_| ());
    }
  }

  /// `mark_live` for the harness scene inside a frame: the dirty set after
  fn marked(h: &mut UiHarness) -> EntityHashSet {
    h.frame_with_world(|ui, world| {
      mark_live(world, ui, ui.scene().0, None);
      world.resource::<UiNodes>().cache.dirty.clone()
    })
  }

  fn note(h: &mut UiHarness, node: Entity) {
    let kind = h.world().get::<UiNodeKind>(node).unwrap().clone();
    note_live(&mut cache_mut(h).cache, node, &kind);
  }

  /// What a record is built under: its environment, scene and frame
  fn built_under(ui: &UiContext<'_>) -> (UiEnv, u64, u64) {
    (ui.env(), ui.scene().0, ui.memory.frame())
  }

  /// A record of `tape` for `node`, built under `under`
  fn insert(h: &mut UiHarness, node: Entity, tape: Tape, under: (UiEnv, u64, u64)) {
    let (env, scene, frame) = under;
    let record = NodeRecord {
      tape,
      scene,
      env,
      top: true,
      frame,
      id: UiId::NONE,
      dialog: false,
    };
    cache_mut(h).cache.records.insert(node, record);
  }

  /// A record of `tape` for `node`, built in a frame of its own
  fn record(h: &mut UiHarness, node: Entity, tape: Tape) {
    let under = h.frame(|ui| built_under(ui));
    insert(h, node, tape, under);
  }

  fn replayable(reads: Vec<Read>) -> Tape {
    Tape {
      replayable: true,
      reads,
      ..Tape::default()
    }
  }

  /// Replayable records of nothing for `nodes`, built in a frame of their
  /// own: `mark_live` marks only nodes with a record
  fn recorded(h: &mut UiHarness, nodes: &[Entity]) {
    let under = h.frame(|ui| built_under(ui));
    for &node in nodes {
      insert(h, node, replayable(Vec::new()), under);
    }
  }

  #[test]
  fn the_roles_of_this_frame_and_the_last_are_marked() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("A").name("a").default_focus())
        .child(UiNode::button("B").name("b")),
    );
    warm(&mut h);
    let (a, b) = (h.find_ui("a").unwrap(), h.find_ui("b").unwrap());
    let (ida, idb) = (h.ui_id(a).unwrap(), h.ui_id(b).unwrap());
    cache_mut(&mut h).cache.by_id.extend([(ida, a), (idb, b)]);
    recorded(&mut h, &[a, b]);
    // A holds the focus: marking now records it as last frame's
    h.frame_with_world(|ui, world| mark_live(world, ui, ui.scene().0, None));
    cache_mut(&mut h).cache.dirty.clear();
    h.action("ui.down"); // the focus moves from A to B
    let moved = marked(&mut h);
    assert!(moved.contains(&a), "A lost the focus: it draws its exit");
    assert!(moved.contains(&b), "B took it");
    cache_mut(&mut h).cache.dirty.clear();
    h.move_to(630.0, 470.0);
    let after = h.frame_with_world(|ui, world| {
      mark_live(world, ui, ui.scene().0, None);
      world.resource_mut::<UiNodes>().cache.dirty.clear();
      // Another scene's pass in the same frame
      mark_live(world, ui, ui.scene().0, None);
      world.resource::<UiNodes>().cache.dirty.clone()
    });
    assert!(
      after.contains(&a),
      "every pass of the frame after still marks last frame's roles"
    );
    cache_mut(&mut h).cache.dirty.clear();
    let still = marked(&mut h);
    assert!(still.contains(&b), "B keeps the focus");
    assert!(!still.contains(&a), "A held no role this frame or the last");
  }

  #[test]
  fn tab_step_marks_tabs_that_hold_no_role() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::tabs(&["One", "Two"], 0).name("tabs"))
        .child(UiNode::button("Other").name("other").default_focus()),
    );
    warm(&mut h);
    let tabs = h.find_ui("tabs").unwrap();
    note(&mut h, tabs);
    recorded(&mut h, &[tabs]);
    assert!(!marked(&mut h).contains(&tabs), "no step: no tab moves");
    h.action("ui.tab_next");
    assert!(marked(&mut h).contains(&tabs));
  }

  #[test]
  fn a_changed_env_makes_a_record_unclean() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(UiNode::column().child(UiNode::button("A").name("a")));
    warm(&mut h);
    let a = h.find_ui("a").unwrap();
    record(&mut h, a, replayable(Vec::new()));
    assert!(
      h.frame_with_world(|ui, world| clean(world, ui, a)),
      "same env: clean"
    );
    h.set_theme(Theme::light());
    assert!(
      !h.frame_with_world(|ui, world| clean(world, ui, a)),
      "another theme: stale"
    );
  }

  #[test]
  fn a_record_is_clean_only_when_replayable_undirty_and_cached() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(UiNode::column().child(UiNode::button("A").name("a")));
    warm(&mut h);
    let a = h.find_ui("a").unwrap();
    let is_clean = |h: &mut UiHarness| h.frame_with_world(|ui, world| clean(world, ui, a));
    assert!(!is_clean(&mut h), "no record");
    record(&mut h, a, Tape::default());
    assert!(!is_clean(&mut h), "not replayable");
    record(&mut h, a, replayable(Vec::new()));
    assert!(is_clean(&mut h));
    cache_mut(&mut h).cache.dirty.insert(a);
    assert!(!is_clean(&mut h), "dirty");
    cache_mut(&mut h).cache.dirty.clear();
    if let Some(record) = cache_mut(&mut h).cache.records.get_mut(&a) {
      record.top = false;
    }
    assert!(!is_clean(&mut h), "built while its scene was below another");
    record(&mut h, a, replayable(Vec::new()));
    cache_mut(&mut h).cache.enabled = false;
    assert!(!is_clean(&mut h), "caching off");
  }

  #[test]
  fn live_kinds_are_marked_every_frame() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column().name("page").child(
        UiNode::scroll()
          .height(100.0)
          .name("area")
          .child(UiNode::button("Row").name("row")),
      ),
    );
    warm(&mut h);
    let [page, area, row] = ["page", "area", "row"].map(|name| h.find_ui(name).unwrap());
    note(&mut h, area);
    note(&mut h, row);
    recorded(&mut h, &[area, row]);
    let live = marked(&mut h);
    assert!(live.contains(&area) && live.contains(&page));
    assert!(!live.contains(&row), "its rows may replay");
    // A node noted as another kind leaves the live set
    cache_mut(&mut h).cache.dirty.clear();
    let button = h.world().get::<UiNodeKind>(row).unwrap().clone();
    note_live(&mut cache_mut(&mut h).cache, area, &button);
    assert!(!marked(&mut h).contains(&area));
  }

  #[test]
  fn an_unfinished_typewriter_is_marked_until_it_settles() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column()
        .child(
          UiNode::text("Welcome, traveller.")
            .typewriter(TextReveal::new(20.0))
            .name("line"),
        )
        .child(UiNode::text("Still").name("still")),
    );
    warm(&mut h);
    let (line, still) = (h.find_ui("line").unwrap(), h.find_ui("still").unwrap());
    recorded(&mut h, &[line, still]);
    let marked_now = marked(&mut h);
    assert!(marked_now.contains(&line), "it reveals more each frame");
    assert!(!marked_now.contains(&still));
    h.world_mut().get_mut::<TextReveal>(line).unwrap().finish();
    h.frame(|_| ());
    cache_mut(&mut h).cache.dirty.clear();
    assert!(
      !marked(&mut h).contains(&line),
      "settled: it draws the same"
    );
  }

  #[test]
  fn the_pending_focus_node_is_marked() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("A").name("a").default_focus())
        .child(UiNode::button("B").name("b")),
    );
    warm(&mut h);
    let (a, b) = (h.find_ui("a").unwrap(), h.find_ui("b").unwrap());
    recorded(&mut h, &[b]);
    let pending = |h: &mut UiHarness, node: Entity| {
      h.frame_with_world(|ui, world| {
        mark_live(world, ui, ui.scene().0, Some(node));
        world.resource::<UiNodes>().cache.dirty.clone()
      })
    };
    assert!(
      pending(&mut h, b).contains(&b),
      "only its build asks for the focus"
    );
    cache_mut(&mut h).cache.dirty.clear();
    assert!(
      !pending(&mut h, a).contains(&a),
      "no record: it is built when drawn"
    );
  }

  #[test]
  fn drop_targets_are_marked_while_a_drag_is_held() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::row()
        .focus_scope()
        .gap(8.0)
        .child(
          UiNode::button("Carry")
            .name("source")
            .drag_source()
            .default_focus(),
        )
        .child(UiNode::button("Here").name("target").drop_target())
        .child(UiNode::button("Other").name("other")),
    );
    warm(&mut h);
    let (target, other) = (h.find_ui("target").unwrap(), h.find_ui("other").unwrap());
    note(&mut h, target);
    note(&mut h, other);
    recorded(&mut h, &[target, other]);
    assert!(!marked(&mut h).contains(&target), "nothing is held");
    h.action("ui.confirm"); // the focused source is picked up
    h.frame(|_| ());
    let (held, marked) = h.frame_with_world(|ui, world| {
      mark_live(world, ui, ui.scene().0, None);
      let dirty = world.resource::<UiNodes>().cache.dirty.clone();
      (ui.held::<Entity>(), dirty)
    });
    assert!(held.is_some(), "a drag is held");
    assert!(marked.contains(&target), "a target may light up");
    assert!(!marked.contains(&other), "not a target");
  }

  /// A difficulty dropdown at `x`
  fn difficulty(ui: &mut UiContext<'_>, x: f32) -> Response {
    let mut selected = 0;
    ui.dropdown(&mut selected, &["Easy", "Normal", "Hard"])
      .id("difficulty")
      .at(Rect::new(x, 40.0, 120.0, 24.0))
      .show()
  }

  #[test]
  fn an_open_dropdown_reads_its_own_rect_and_rebuilds_once_it_moved() {
    let mut h = UiHarness::new(640.0, 480.0);
    let node = h.spawn_ui(UiNode::column().name("page"));
    warm(&mut h);
    let trigger = h.frame(|ui| difficulty(ui, 10.0)).rect;
    h.move_to(trigger.x + 5.0, trigger.y + 5.0);
    h.press();
    h.frame(|ui| difficulty(ui, 10.0));
    h.release();
    h.frame(|ui| difficulty(ui, 10.0)); // the click opens it
    let (tape, id, under) = h.frame(|ui| {
      ui.start_tape();
      let id = difficulty(ui, 10.0).id;
      (ui.end_tape(), id, built_under(ui))
    });
    let rect = h.record_of(id).map(|record| record.rect);
    assert!(
      tape.reads.contains(&Read::Rect { id, rect }),
      "its list's height comes from its own rect: {:?}",
      tape.reads
    );
    // Closed, it places nothing by itself
    let closed = {
      let mut h = UiHarness::new(640.0, 480.0);
      h.frame(|ui| {
        ui.start_tape();
        difficulty(ui, 10.0);
        ui.end_tape()
      })
    };
    assert!(closed.reads.is_empty());

    insert(&mut h, node, replayable(tape.reads.clone()), under);
    let is_clean = |h: &mut UiHarness, x: f32| {
      h.frame_with_world(|ui, world| {
        let clean = clean(world, ui, node);
        difficulty(ui, x);
        clean
      })
    };
    assert!(is_clean(&mut h, 10.0), "where it was: clean");
    h.frame(|ui| difficulty(ui, 200.0));
    assert!(!is_clean(&mut h, 200.0), "it moved: its list is rebuilt");
  }

  #[test]
  fn a_node_reads_its_neighbours_and_its_anchors_ids() {
    let mut h = UiHarness::new(640.0, 480.0);
    let root = h.spawn_ui(
      UiNode::column()
        .embedded()
        .focus_scope()
        .child(UiNode::button("A").name("a").nav_down("more"))
        .child(UiNode::button("More").name("more"))
        .child(UiNode::popup("more").child(UiNode::button("Rename").name("rename"))),
    );
    let draw = |h: &mut UiHarness| {
      h.frame_with_world(|ui, world| {
        ui.start_tape();
        ui.node(root, world);
        ui.end_tape()
      })
    };
    for _ in 0..3 {
      draw(&mut h);
    }
    let tape = draw(&mut h);
    let more = h.find_ui("more").unwrap();
    let id = h.ui_id(more).unwrap();
    let read = Read::Id { node: more, id };
    assert_eq!(
      tape.reads.iter().filter(|r| **r == read).count(),
      2,
      "A's nav_down and the popup's anchor: {:?}",
      tape.reads
    );

    let a = h.find_ui("a").unwrap();
    record(&mut h, a, replayable(vec![read]));
    let is_clean = |h: &mut UiHarness| h.frame_with_world(|ui, world| clean(world, ui, a));
    assert!(is_clean(&mut h), "the neighbour's id is as read");
    if let Some(mut state) = h.world_mut().get_mut::<UiInteraction>(more) {
      state.id = UiId(12_345);
    }
    assert!(!is_clean(&mut h), "the neighbour's id changed");
  }

  #[test]
  fn a_node_without_a_record_of_this_scene_is_not_marked() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column().name("page").child(
        UiNode::column().name("branch").hidden().child(
          UiNode::scroll()
            .height(100.0)
            .name("area")
            .child(UiNode::text("Hello")),
        ),
      ),
    );
    warm(&mut h);
    let (page, area) = (h.find_ui("page").unwrap(), h.find_ui("area").unwrap());
    note(&mut h, area);
    let unseen = marked(&mut h);
    assert!(
      !unseen.contains(&area) && !unseen.contains(&page),
      "a live node under a hidden branch rebuilds nothing above it"
    );
    let (env, scene, frame) = h.frame(|ui| built_under(ui));
    insert(
      &mut h,
      area,
      replayable(Vec::new()),
      (env, scene + 1, frame),
    );
    assert!(
      !marked(&mut h).contains(&area),
      "its record is another scene's"
    );
  }

  /// A column holding the difficulty dropdown at `x`, each taped as the
  /// walk tapes a node and its child: the column's tape, the dropdown's,
  /// and the dropdown's response
  fn taped_column(ui: &mut UiContext<'_>, x: f32) -> (Tape, Tape, Response) {
    let slot = Entity::from_raw_u32(7).unwrap();
    let mut inner = (Tape::default(), Response::default());
    ui.start_tape();
    ui.column().id("column").show(|ui| {
      ui.begin_child(slot);
      ui.start_tape();
      let response = difficulty(ui, x);
      inner = (ui.end_tape(), response);
      ui.end_child();
    });
    (ui.end_tape(), inner.0, inner.1)
  }

  #[test]
  fn a_parent_carries_the_reads_of_its_subtree() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::row()
        .child(UiNode::column().name("left"))
        .child(UiNode::column().name("right")),
    );
    warm(&mut h);
    let id = h.frame(|ui| taped_column(ui, 10.0).2.id);
    let trigger = h.record_of(id).unwrap().rect;
    h.move_to(trigger.x + 5.0, trigger.y + 5.0);
    h.press();
    h.frame(|ui| taped_column(ui, 10.0));
    h.release();
    h.frame(|ui| taped_column(ui, 10.0)); // the click opens it
    let (column, dropdown, under) = h.frame(|ui| {
      let (column, dropdown, _) = taped_column(ui, 10.0);
      (column, dropdown, built_under(ui))
    });
    let read = Read::Rect {
      id,
      rect: h.record_of(id).map(|record| record.rect),
    };
    assert!(dropdown.reads.contains(&read), "{:?}", dropdown.reads);
    assert_eq!(column.reads, [read], "the column carries its child's read");

    // The column's record on one node, a sibling's on another
    let (left, right) = (h.find_ui("left").unwrap(), h.find_ui("right").unwrap());
    insert(&mut h, left, replayable(column.reads.clone()), under);
    insert(&mut h, right, replayable(Vec::new()), under);
    let both = |h: &mut UiHarness, x: f32| {
      h.frame_with_world(|ui, world| {
        let both = (clean(world, ui, left), clean(world, ui, right));
        taped_column(ui, x);
        both
      })
    };
    assert_eq!(both(&mut h, 10.0), (true, true));
    h.frame(|ui| taped_column(ui, 200.0));
    assert_eq!(
      both(&mut h, 200.0),
      (false, true),
      "the column around the moved dropdown rebuilds; its sibling replays"
    );

    // Replayed inside a recording tape, the column's reads come along once:
    // they already hold the dropdown's
    let slot = Entity::from_raw_u32(8).unwrap();
    let outer = h.frame(|ui| {
      ui.start_tape();
      ui.column().id("outer").show(|ui| {
        ui.begin_child(slot);
        ui.replay(&column, &mut |ui, _| ui.replay(&dropdown, &mut |_, _| {}));
        ui.end_child();
      });
      ui.end_tape()
    });
    assert_eq!(outer.reads, column.reads);
  }

  #[test]
  fn a_number_key_marks_a_choice_list_that_holds_no_role() {
    let mut d = UiDriver::new((640.0, 480.0));
    // Embedded, so each frame marks before the tree is drawn, as the walk
    // will: the list takes the key it picks with
    let root = d.spawn_ui(
      UiNode::column()
        .embedded()
        .focus_scope()
        .child(UiNode::button("Other").name("other").default_focus())
        .child(UiNode::choice_list(&["Yes", "No"]).name("choices")),
    );
    let frame = |d: &mut UiDriver| {
      d.frame_with_world(|ui, world| {
        mark_live(world, ui, ui.scene().0, None);
        let marked = world.resource::<UiNodes>().cache.dirty.clone();
        ui.node(root, world);
        (marked, built_under(ui))
      })
    };
    for _ in 0..3 {
      frame(&mut d);
    }
    let choices = d.find_ui("choices").unwrap();
    note(&mut d, choices);
    let (_, under) = frame(&mut d);
    insert(&mut d, choices, replayable(Vec::new()), under);
    assert!(!frame(&mut d).0.contains(&choices), "no number key");
    d.key(KeyCode::Digit2);
    assert!(
      frame(&mut d).0.contains(&choices),
      "2 picks its second choice"
    );
  }

  #[test]
  fn a_rebinding_row_is_marked_while_it_shows_a_note() {
    let prompts = prompts_with(
      InputDevice::Keyboard,
      &[("jump", vec![Binding::Key(KeyCode::Space)])],
    );
    let mut d = UiDriver::new((640.0, 480.0)).prompts(prompts);
    d.spawn_ui(UiNode::column().child(UiNode::rebind("jump").name("row")));
    warm(&mut d);
    let row = d.find_ui("row").unwrap();
    note(&mut d, row);
    recorded(&mut d, &[row]);
    assert!(!marked(&mut d).contains(&row), "idle");
    let (id, scene) = (d.ui_id(row).unwrap(), d.frame(|ui| ui.scene().0));
    let now = d.memory.interact.now;
    if let Some(state) = d.memory.state::<RebindState>(id, scene) {
      // A swap the engine applied after the pass: its note fades on time
      state.note = Some(("Swapped with run".to_string(), now));
    }
    assert!(marked(&mut d).contains(&row));
  }

  #[test]
  fn a_text_field_sized_by_layout_reads_its_own_rect() {
    let field = |ui: &mut UiContext<'_>, fill: bool| {
      let mut name = String::from("Ada");
      let mut tape = Tape::default();
      let mut id = UiId::NONE;
      ui.column().id("form").width(300.0).show(|ui| {
        ui.start_tape();
        let mut builder = ui.text_input(&mut name).id("name");
        if fill {
          builder = builder.fill_width();
        }
        id = builder.show().id;
        tape = ui.end_tape();
      });
      (tape, id)
    };
    let mut h = UiHarness::new(640.0, 480.0);
    h.frame(|ui| field(ui, true));
    h.frame(|ui| field(ui, true));
    let (tape, id) = h.frame(|ui| field(ui, true));
    let rect = h.record_of(id).map(|record| record.rect);
    assert_eq!(
      tape.reads,
      [Read::Rect { id, rect }],
      "it fills: its text is placed in last frame's rect"
    );
    let (fixed, _) = h.frame(|ui| field(ui, false));
    assert!(fixed.reads.is_empty(), "its own size: nothing read");
  }
}
