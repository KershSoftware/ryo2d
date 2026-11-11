//! The occluder index: every enabled occluder's world bounds in a uniform
//! grid of cells, so a light reads only the occluders near it.
//!
//! It is kept up to date in `PostUpdate`, after transforms propagate, from
//! what changed since its last update: occluders added or moved (their
//! `GlobalTransform`, which a parent's move reaches too) or edited (their
//! `LightOccluder2D`, `set_enabled` among it), taken away or despawned, and
//! disabled or enabled through the ECS's `Disabled` (watched by observers, as
//! no change filter shows it). A frame where none of that happened touches
//! no entry.
//!
//! An interpolated occluder is drawn between its previous and its current
//! placement (`render_global`), so its bounds hold both, padded for the arc
//! a turn sweeps; it is checked again each frame until its previous
//! placement catches up with its current one, when its bounds shrink back.

use std::f32::consts::{PI, TAU};
use std::sync::{Mutex, MutexGuard, TryLockError};

use glam::{IVec2, Vec2};

use super::LightOccluder2D;
use crate::core::ecs::kernel::{Add, Remove};
use crate::core::ecs::{
  Allow, Changed, Disabled, Entity, EntityHashMap, GlobalTransform, Has, On, Or,
  PreviousGlobalTransform, Query, RemovedComponents, ResMut, Resource, With, World, entity_order,
};
use crate::core::hash::FastMap;

/// The side of an index cell in world units: a tile map chunk of 32 cells
/// of 16 px, and about a typical light's reach
pub(crate) const INDEX_CELL: f32 = 512.0;

/// The most cells an entry is listed in. One covering more (an occluder of
/// over 16,000 px) is kept with the large entries, which every query checks
const MAX_ENTRY_CELLS: i64 = 1024;

/// The most emptied cells' vectors kept for reuse
const MAX_SPARE_CELLS: usize = 256;

/// A rectangle of cells, both corners included
#[derive(Clone, Copy, PartialEq, Debug)]
struct CellRange {
  min: IVec2,
  max: IVec2,
}

impl CellRange {
  /// The cells a box from `min` to `max` (finite) covers
  fn of(min: Vec2, max: Vec2) -> CellRange {
    // Saturating: a box beyond the cells' range is clamped to its edge
    CellRange {
      min: (min / INDEX_CELL).floor().as_ivec2(),
      max: (max / INDEX_CELL).floor().as_ivec2(),
    }
  }

  fn count(self) -> i64 {
    let span = |a: i32, b: i32| i64::from(b) - i64::from(a) + 1;
    span(self.min.x, self.max.x) * span(self.min.y, self.max.y)
  }

  fn contains(self, cell: IVec2) -> bool {
    cell.cmpge(self.min).all() && cell.cmple(self.max).all()
  }

  fn cells(self) -> impl Iterator<Item = IVec2> {
    (self.min.y..=self.max.y)
      .flat_map(move |y| (self.min.x..=self.max.x).map(move |x| IVec2::new(x, y)))
  }
}

/// Where an occluder is to be found
#[derive(Clone, Copy, PartialEq, Debug)]
struct Placement {
  min: Vec2,
  max: Vec2,
  /// Its bounds hold its previous placement as well as its current one
  moving: bool,
}

/// One occluder's entry
#[derive(Debug)]
struct Entry {
  entity: Entity,
  min: Vec2,
  max: Vec2,
  /// The cells it is listed in; `None` for a large entry
  cells: Option<CellRange>,
  /// Listed in the index's `moving`, to be checked each frame
  moving: bool,
}

/// What a query keeps between calls, so it allocates nothing once warm: the
/// entities found, and a mark per entry of the last query that found it
#[derive(Default)]
struct Scratch {
  found: Vec<Entity>,
  marks: Vec<u32>,
  generation: u32,
}

/// Every enabled occluder's world bounds, by entity and by the cells they
/// cover
#[derive(Resource, Default)]
pub(crate) struct OccluderIndex {
  /// The entries by slot; a free slot is `None`
  entries: Vec<Option<Entry>>,
  free: Vec<u32>,
  slot_of: EntityHashMap<u32>,
  /// The slots listed in each cell; a cell with none is not kept
  cells: FastMap<IVec2, Vec<u32>>,
  /// Emptied cells' vectors, for the next cells
  spare: Vec<Vec<u32>>,
  /// The slots of entries covering more than `MAX_ENTRY_CELLS`
  large: Vec<u32>,
  /// The entities whose entries hold a previous placement
  moving: Vec<Entity>,
  /// Last update's `moving`, its vector kept
  recheck: Vec<Entity>,
  /// Entities disabled or enabled since the last update
  toggled: Vec<Entity>,
  scratch: Mutex<Scratch>,
  #[cfg(test)]
  updates: u64,
}

/// What a query found, held while it is walked
enum Held<'a> {
  Shared(MutexGuard<'a, Scratch>),
  /// Found while another query held the scratch
  Owned(Vec<Entity>),
}

struct Found<'a> {
  held: Held<'a>,
  next: usize,
}

impl Iterator for Found<'_> {
  type Item = Entity;

  fn next(&mut self) -> Option<Entity> {
    let found = match &self.held {
      Held::Shared(scratch) => &scratch.found,
      Held::Owned(found) => found,
    };
    let entity = found.get(self.next).copied();
    self.next += 1;
    entity
  }
}

impl OccluderIndex {
  /// The occluders whose bounds meet the box from `min` to `max`, each
  /// once, in entity order (`entity_order`); none for a box that is not
  /// finite. Allocates nothing once warm, unless called while another
  /// query's result is still being walked
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "shadow planning will read the index")
  )]
  pub(crate) fn query(&self, min: Vec2, max: Vec2) -> impl Iterator<Item = Entity> + '_ {
    let mut held = match self.scratch.try_lock() {
      Ok(scratch) => Held::Shared(scratch),
      Err(TryLockError::Poisoned(poisoned)) => Held::Shared(poisoned.into_inner()),
      Err(TryLockError::WouldBlock) => Held::Owned(Vec::new()),
    };
    match &mut held {
      Held::Shared(scratch) => {
        let Scratch {
          found,
          marks,
          generation,
        } = &mut **scratch;
        found.clear();
        if marks.len() < self.entries.len() {
          marks.resize(self.entries.len(), 0);
        }
        *generation = generation.wrapping_add(1);
        if *generation == 0 {
          marks.fill(0);
          *generation = 1;
        }
        let generation = *generation;
        self.gather(min, max, |slot, entity| {
          if marks[slot] != generation {
            marks[slot] = generation;
            found.push(entity);
          }
        });
        found.sort_unstable_by_key(entity_order);
      }
      Held::Owned(found) => {
        self.gather(min, max, |_, entity| found.push(entity));
        found.sort_unstable_by_key(entity_order);
        found.dedup();
      }
    }
    Found { held, next: 0 }
  }

  /// The bounds the index keeps for `entity`, as (min, max); `None` for an
  /// entity it does not hold
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "shadow planning will read the index")
  )]
  pub(crate) fn bounds(&self, entity: Entity) -> Option<(Vec2, Vec2)> {
    self.entry(entity).map(|entry| (entry.min, entry.max))
  }

  /// Entries touched since the index was made
  #[cfg(test)]
  pub(crate) fn updates(&self) -> u64 {
    self.updates
  }

  fn touched(&mut self) {
    #[cfg(test)]
    {
      self.updates += 1;
    }
  }

  fn entry(&self, entity: Entity) -> Option<&Entry> {
    let slot = *self.slot_of.get(&entity)?;
    self.entries.get(slot as usize)?.as_ref()
  }

  fn entry_mut(&mut self, entity: Entity) -> Option<&mut Entry> {
    let slot = *self.slot_of.get(&entity)?;
    self.entries.get_mut(slot as usize)?.as_mut()
  }

  /// Calls `visit` with the slot and entity of every entry whose bounds
  /// meet the box, an entry perhaps more than once
  fn gather(&self, min: Vec2, max: Vec2, mut visit: impl FnMut(usize, Entity)) {
    if !(min.is_finite() && max.is_finite()) || min.cmpgt(max).any() {
      return;
    }
    let mut check = |slot: u32| {
      if let Some(Some(entry)) = self.entries.get(slot as usize)
        && entry.min.cmple(max).all()
        && entry.max.cmpge(min).all()
      {
        visit(slot as usize, entry.entity);
      }
    };
    for &slot in &self.large {
      check(slot);
    }
    let range = CellRange::of(min, max);
    // A box over more cells than are kept walks the kept ones instead
    if range.count() <= self.cells.len() as i64 {
      for cell in range.cells() {
        for &slot in self.cells.get(&cell).into_iter().flatten() {
          check(slot);
        }
      }
    } else {
      for (_, slots) in self.cells.iter().filter(|(cell, _)| range.contains(**cell)) {
        for &slot in slots {
          check(slot);
        }
      }
    }
  }

  /// Gives `entity` the entry `placement` asks for (none for `None`),
  /// touching it only if it differs from what is kept
  fn place(&mut self, entity: Entity, placement: Option<Placement>) {
    let slot = self.slot_of.get(&entity).copied();
    match (slot, placement) {
      (None, None) => {}
      (Some(slot), None) => self.remove(entity, slot),
      (None, Some(placement)) => self.insert(entity, placement),
      (Some(slot), Some(placement)) => self.update(entity, slot, placement),
    }
  }

  fn insert(&mut self, entity: Entity, placement: Placement) {
    let cells = listed_cells(placement);
    let slot = match self.free.pop() {
      Some(slot) => slot,
      None => {
        self.entries.push(None);
        (self.entries.len() - 1) as u32
      }
    };
    self.entries[slot as usize] = Some(Entry {
      entity,
      min: placement.min,
      max: placement.max,
      cells,
      moving: placement.moving,
    });
    self.slot_of.insert(entity, slot);
    if placement.moving {
      self.moving.push(entity);
    }
    match cells {
      Some(cells) => cells.cells().for_each(|cell| self.list(slot, cell)),
      None => self.large.push(slot),
    }
    self.touched();
  }

  fn remove(&mut self, entity: Entity, slot: u32) {
    self.slot_of.remove(&entity);
    let Some(entry) = self.entries.get_mut(slot as usize).and_then(Option::take) else {
      return;
    };
    self.free.push(slot);
    match entry.cells {
      Some(cells) => cells.cells().for_each(|cell| self.unlist(slot, cell)),
      None => self.large.retain(|&s| s != slot),
    }
    self.touched();
  }

  fn update(&mut self, entity: Entity, slot: u32, placement: Placement) {
    let cells = listed_cells(placement);
    let Some(Some(entry)) = self.entries.get_mut(slot as usize) else {
      return;
    };
    let newly_moving = placement.moving && !entry.moving;
    entry.moving |= placement.moving;
    let same = entry.min == placement.min && entry.max == placement.max;
    let old = entry.cells;
    if !same {
      entry.min = placement.min;
      entry.max = placement.max;
      entry.cells = cells;
    }
    if newly_moving {
      self.moving.push(entity);
    }
    if same {
      return;
    }
    match (old, cells) {
      (Some(old), Some(new)) => {
        if old != new {
          for cell in old.cells().filter(|&cell| !new.contains(cell)) {
            self.unlist(slot, cell);
          }
          for cell in new.cells().filter(|&cell| !old.contains(cell)) {
            self.list(slot, cell);
          }
        }
      }
      (old, new) => {
        match old {
          Some(old) => old.cells().for_each(|cell| self.unlist(slot, cell)),
          None => self.large.retain(|&s| s != slot),
        }
        match new {
          Some(new) => new.cells().for_each(|cell| self.list(slot, cell)),
          None => self.large.push(slot),
        }
      }
    }
    self.touched();
  }

  fn list(&mut self, slot: u32, cell: IVec2) {
    let spare = &mut self.spare;
    self
      .cells
      .entry(cell)
      .or_insert_with(|| spare.pop().unwrap_or_default())
      .push(slot);
  }

  fn unlist(&mut self, slot: u32, cell: IVec2) {
    let Some(slots) = self.cells.get_mut(&cell) else {
      return;
    };
    if let Some(at) = slots.iter().position(|&s| s == slot) {
      slots.swap_remove(at);
    }
    if slots.is_empty()
      && let Some(emptied) = self.cells.remove(&cell)
      && self.spare.len() < MAX_SPARE_CELLS
    {
      self.spare.push(emptied);
    }
  }
}

/// The cells an entry of `placement` is listed in; `None` if it covers
/// more than an entry may (a large entry)
fn listed_cells(placement: Placement) -> Option<CellRange> {
  let cells = CellRange::of(placement.min, placement.max);
  (cells.count() <= MAX_ENTRY_CELLS).then_some(cells)
}

/// The shortest turn from `from` to `to`, in radians (0 to pi)
fn turn_between(from: f32, to: f32) -> f32 {
  let mut delta = to - from;
  if delta > PI {
    delta -= TAU;
  } else if delta < -PI {
    delta += TAU;
  }
  delta.abs()
}

/// What placing an occluder reads, disabled ones too
type OccluderItem = (
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  &'static LightOccluder2D,
  Has<Disabled>,
);

/// The occluders moved, added or edited
type OccluderChanged = (
  With<LightOccluder2D>,
  Or<(Changed<GlobalTransform>, Changed<LightOccluder2D>)>,
);

type OccluderData<'a> = (
  &'a GlobalTransform,
  Option<&'a PreviousGlobalTransform>,
  &'a LightOccluder2D,
  bool,
);

/// Where an occluder is to be found: none while it is disabled, or placed
/// where a vertex is not finite. An interpolated one between two
/// placements holds both, padded by the most a turn's arc bows out from
/// the line between a vertex's two places (the extent times 1 - cos of
/// half the turn), so every blend `render_global` draws lies inside
fn placement((global, previous, occluder, disabled): OccluderData) -> Option<Placement> {
  if disabled || !occluder.is_enabled() {
    return None;
  }
  let (mut min, mut max) = occluder.placed_bounds(global)?;
  let mut moving = false;
  if let Some(previous) = previous.filter(|previous| !previous.skip) {
    let from = previous.global;
    let turn = turn_between(from.rotation(), global.rotation());
    if from.translation() != global.translation() || turn != 0.0 {
      let (was_min, was_max) = occluder.placed_bounds(&from)?;
      let bow = occluder.extent() * (1.0 - (turn * 0.5).cos());
      min = min.min(was_min) - bow;
      max = max.max(was_max) + bow;
      moving = true;
    }
  }
  Some(Placement { min, max, moving })
}

/// Inserts the index and the observers that queue occluders being disabled
/// and enabled for its next update
pub(crate) fn insert_occluder_index(world: &mut World) {
  world.insert_resource(OccluderIndex::default());
  world.add_observer(occluder_disabled);
  world.add_observer(occluder_enabled);
}

/// An entity is being disabled: an occluder on it leaves the index at its
/// next update
fn occluder_disabled(
  disabled: On<Add<Disabled>>,
  occluders: Query<(), (With<LightOccluder2D>, Allow<Disabled>)>,
  index: Option<ResMut<OccluderIndex>>,
) {
  if occluders.contains(disabled.entity)
    && let Some(mut index) = index
  {
    index.toggled.push(disabled.entity);
  }
}

/// An entity is being enabled (or despawned while disabled): an occluder on
/// it is placed again at the index's next update
fn occluder_enabled(
  enabled: On<Remove<Disabled>>,
  occluders: Query<(), (With<LightOccluder2D>, Allow<Disabled>)>,
  index: Option<ResMut<OccluderIndex>>,
) {
  if occluders.contains(enabled.entity)
    && let Some(mut index) = index
  {
    index.toggled.push(enabled.entity);
  }
}

/// Brings the index up to date with the occluders that changed since its
/// last update (`PostUpdate`, after transforms propagate and after the tile
/// maps' occluders are built, so a map edit's chains are in it that frame)
pub(crate) fn update_occluder_index(
  index: ResMut<OccluderIndex>,
  changed: Query<Entity, OccluderChanged>,
  occluders: Query<OccluderItem, Allow<Disabled>>,
  mut removed: RemovedComponents<LightOccluder2D>,
  mut unplaced: RemovedComponents<GlobalTransform>,
) {
  let index = index.into_inner();
  // Last update's moving entries are checked again: their previous
  // placement may have caught up with their current one
  let mut recheck = std::mem::take(&mut index.recheck);
  recheck.clear();
  std::mem::swap(&mut recheck, &mut index.moving);
  for &entity in &recheck {
    if let Some(entry) = index.entry_mut(entity) {
      entry.moving = false;
    }
  }
  let refresh = |index: &mut OccluderIndex, entity| {
    index.place(entity, occluders.get(entity).ok().and_then(placement))
  };
  for entity in removed.read() {
    refresh(index, entity);
  }
  for entity in unplaced.read() {
    refresh(index, entity);
  }
  let mut toggled = std::mem::take(&mut index.toggled);
  for entity in toggled.drain(..) {
    refresh(index, entity);
  }
  index.toggled = toggled;
  for entity in &changed {
    refresh(index, entity);
  }
  for &entity in &recheck {
    refresh(index, entity);
  }
  index.recheck = recheck;
  let entries = index.entries.len();
  if let Ok(scratch) = index.scratch.get_mut()
    && scratch.marks.len() < entries
  {
    scratch.marks.resize(entries, 0);
  }
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;
  use std::sync::Arc;

  use glam::UVec2;

  use super::*;
  use crate::core::asset::Assets;
  use crate::core::ecs::interpolate::{render_global, snapshot_previous_transforms};
  use crate::core::ecs::propagate::propagate_transforms;
  use crate::core::ecs::{Interpolated, IntoScheduleConfigs, Transform, entity_order};
  use crate::core::schedule::{EngineSchedules, FixedSet, Phase, PostUpdateSet};
  use crate::core::tilemap::test_util::MapWorld;
  use crate::core::tilemap::{
    MapOccluder, MapRoot, MapsView, Tile, TileData, TileMap, TileShape, Tileset, TilesetImage,
    TilesetSource,
  };

  /// A world with the index, its observers and system, transform
  /// propagation and interpolation's snapshot
  struct Lit {
    world: World,
    schedules: EngineSchedules,
  }

  impl Lit {
    fn new() -> Lit {
      let mut world = World::new();
      insert_occluder_index(&mut world);
      let mut schedules = EngineSchedules::new();
      add_index_systems(&mut schedules);
      schedules.add_systems(
        Phase::FixedUpdate,
        snapshot_previous_transforms.in_set(FixedSet::Snapshot),
      );
      schedules.add_systems(
        Phase::FixedUpdate,
        propagate_transforms.in_set(FixedSet::Transforms),
      );
      schedules.add_systems(
        Phase::PostUpdate,
        propagate_transforms.in_set(PostUpdateSet::Transforms),
      );
      Lit { world, schedules }
    }

    /// A frame without a fixed step
    fn frame(&mut self) {
      self.schedules.run(Phase::PostUpdate, &mut self.world);
      self.world.clear_trackers();
    }

    /// A frame with one fixed step
    fn step(&mut self) {
      self.schedules.run(Phase::FixedUpdate, &mut self.world);
      self.frame();
    }

    fn index(&self) -> &OccluderIndex {
      self.world.resource::<OccluderIndex>()
    }

    fn found(&self, min: Vec2, max: Vec2) -> Vec<Entity> {
      self.index().query(min, max).collect()
    }

    /// The occluders found in a 2 px box around `at`
    fn found_at(&self, at: Vec2) -> Vec<Entity> {
      self.found(at - Vec2::ONE, at + Vec2::ONE)
    }

    fn spawn(&mut self, x: f32, y: f32, occluder: LightOccluder2D) -> Entity {
      self.world.spawn((Transform::from_xy(x, y), occluder)).id()
    }

    fn move_to(&mut self, entity: Entity, x: f32, y: f32) {
      if let Some(mut transform) = self.world.get_mut::<Transform>(entity) {
        transform.translation = Vec2::new(x, y);
      }
    }
  }

  /// The engine's registration of the index's system
  fn add_index_systems(schedules: &mut EngineSchedules) {
    schedules.add_systems(
      Phase::PostUpdate,
      update_occluder_index.after(PostUpdateSet::Transforms),
    );
  }

  /// How many entries a cell holds (0 for one the index does not keep)
  fn cell_len(index: &OccluderIndex, cell: IVec2) -> usize {
    index.cells.get(&cell).map_or(0, Vec::len)
  }

  #[test]
  fn an_added_occluder_is_found_where_it_stands() {
    let mut lit = Lit::new();
    let wall = lit.spawn(1000.0, 300.0, LightOccluder2D::rect(40.0, 40.0));
    lit.frame();
    assert_eq!(lit.found_at(Vec2::new(1000.0, 300.0)), vec![wall]);
    assert_eq!(lit.found_at(Vec2::new(1019.0, 319.0)), vec![wall]);
    assert!(lit.found_at(Vec2::new(1030.0, 300.0)).is_empty());
    assert!(lit.found_at(Vec2::ZERO).is_empty());
    assert_eq!(
      lit.index().bounds(wall),
      Some((Vec2::new(980.0, 280.0), Vec2::new(1020.0, 320.0)))
    );
    // A box that is not finite finds nothing
    let nan = Vec2::new(f32::NAN, 0.0);
    assert!(lit.found(nan, Vec2::splat(2000.0)).is_empty());
    assert!(
      lit
        .found(Vec2::splat(f32::NEG_INFINITY), Vec2::splat(f32::INFINITY))
        .is_empty()
    );
    // A rotated occluder is boxed as it is placed
    let turned = lit
      .world
      .spawn((
        Transform::from_xy(-2000.0, 0.0).with_rotation(FRAC_PI_2),
        LightOccluder2D::rect(100.0, 10.0),
      ))
      .id();
    lit.frame();
    let Some((min, max)) = lit.index().bounds(turned) else {
      panic!("the turned occluder is indexed");
    };
    assert!(
      (min - Vec2::new(-2005.0, -50.0)).abs().max_element() < 1e-3,
      "{min}"
    );
    assert!(
      (max - Vec2::new(-1995.0, 50.0)).abs().max_element() < 1e-3,
      "{max}"
    );
  }

  #[test]
  fn a_moved_occluder_leaves_its_old_cells() {
    let mut lit = Lit::new();
    let wall = lit.spawn(1000.0, 300.0, LightOccluder2D::rect(40.0, 40.0));
    lit.frame();
    assert_eq!(cell_len(lit.index(), IVec2::new(1, 0)), 1);
    lit.move_to(wall, 3000.0, -700.0);
    lit.frame();
    assert!(lit.found_at(Vec2::new(1000.0, 300.0)).is_empty());
    assert_eq!(lit.found_at(Vec2::new(3000.0, -700.0)), vec![wall]);
    assert_eq!(cell_len(lit.index(), IVec2::new(1, 0)), 0);
    assert_eq!(cell_len(lit.index(), IVec2::new(5, -2)), 1);
    // Moved by its parent
    let parent = lit.world.spawn(Transform::from_xy(0.0, 0.0)).id();
    let child = lit
      .world
      .spawn((
        Transform::from_xy(10.0, 0.0),
        LightOccluder2D::rect(4.0, 4.0),
        crate::core::ecs::ChildOf(parent),
      ))
      .id();
    lit.frame();
    assert_eq!(lit.found_at(Vec2::new(10.0, 0.0)), vec![child]);
    lit.move_to(parent, 5000.0, 0.0);
    lit.frame();
    assert!(lit.found_at(Vec2::new(10.0, 0.0)).is_empty());
    assert_eq!(lit.found_at(Vec2::new(5010.0, 0.0)), vec![child]);
  }

  #[test]
  fn a_removed_or_despawned_occluder_is_gone() {
    let mut lit = Lit::new();
    let taken = lit.spawn(100.0, 100.0, LightOccluder2D::rect(40.0, 40.0));
    let despawned = lit.spawn(110.0, 100.0, LightOccluder2D::rect(40.0, 40.0));
    lit.frame();
    assert_eq!(lit.found_at(Vec2::new(105.0, 100.0)).len(), 2);
    lit.world.entity_mut(taken).remove::<LightOccluder2D>();
    lit.world.despawn(despawned);
    lit.frame();
    assert!(lit.found_at(Vec2::new(105.0, 100.0)).is_empty());
    assert_eq!(lit.index().bounds(taken), None);
    assert_eq!(lit.index().bounds(despawned), None);
    assert_eq!(cell_len(lit.index(), IVec2::ZERO), 0);
    // Given back, it is found again
    lit
      .world
      .entity_mut(taken)
      .insert(LightOccluder2D::rect(40.0, 40.0));
    lit.frame();
    assert_eq!(lit.found_at(Vec2::new(105.0, 100.0)), vec![taken]);
  }

  #[test]
  fn disabling_and_re_enabling_an_occluder_is_followed() {
    let mut lit = Lit::new();
    let wall = lit.spawn(100.0, 100.0, LightOccluder2D::rect(40.0, 40.0));
    lit.frame();
    let at = Vec2::new(100.0, 100.0);
    assert_eq!(lit.found_at(at), vec![wall]);
    let set_enabled = |lit: &mut Lit, on: bool| {
      if let Some(mut occluder) = lit.world.get_mut::<LightOccluder2D>(wall) {
        occluder.set_enabled(on);
      }
    };
    set_enabled(&mut lit, false);
    lit.frame();
    assert!(lit.found_at(at).is_empty());
    assert_eq!(lit.index().bounds(wall), None);
    set_enabled(&mut lit, true);
    lit.frame();
    assert_eq!(lit.found_at(at), vec![wall]);
    // The ECS's own disabling, which no change filter shows
    lit.world.entity_mut(wall).insert(Disabled);
    lit.frame();
    assert!(lit.found_at(at).is_empty());
    assert_eq!(lit.index().bounds(wall), None);
    lit.world.entity_mut(wall).remove::<Disabled>();
    lit.frame();
    assert_eq!(lit.found_at(at), vec![wall]);
    // Spawned disabled (a pool's batch), then enabled
    let pooled = lit
      .world
      .spawn((
        Transform::from_xy(-300.0, 0.0),
        LightOccluder2D::rect(4.0, 4.0),
        Disabled,
      ))
      .id();
    lit.frame();
    assert!(lit.found_at(Vec2::new(-300.0, 0.0)).is_empty());
    lit.world.entity_mut(pooled).remove::<Disabled>();
    lit.frame();
    assert_eq!(lit.found_at(Vec2::new(-300.0, 0.0)), vec![pooled]);
    // Despawned while disabled
    lit.world.entity_mut(pooled).insert(Disabled);
    lit.frame();
    lit.world.despawn(pooled);
    lit.frame();
    assert_eq!(lit.index().bounds(pooled), None);
    assert_eq!(lit.found_at(at), vec![wall]);
  }

  #[test]
  fn a_big_occluder_is_found_from_every_cell_it_covers() {
    let mut lit = Lit::new();
    let small = lit.spawn(-4000.0, -4000.0, LightOccluder2D::rect(10.0, 10.0));
    let big = lit.spawn(0.0, 0.0, LightOccluder2D::rect(10_000.0, 10_000.0));
    lit.frame();
    for at in [
      Vec2::new(-4990.0, -4990.0),
      Vec2::new(4990.0, -4990.0),
      Vec2::new(-4990.0, 4990.0),
      Vec2::new(4990.0, 4990.0),
      Vec2::ZERO,
    ] {
      assert_eq!(lit.found_at(at), vec![big], "at {at}");
    }
    // A box over many of its cells finds it once, with the small one, in
    // entity order
    let mut both = vec![small, big];
    both.sort_by_key(entity_order);
    assert_eq!(lit.found(Vec2::splat(-6000.0), Vec2::splat(6000.0)), both);
    assert!(lit.found_at(Vec2::new(5010.0, 0.0)).is_empty());
  }

  #[test]
  fn a_huge_occluder_is_kept_with_the_large_ones() {
    let mut lit = Lit::new();
    let huge = lit.spawn(0.0, 0.0, LightOccluder2D::rect(2.0e7, 2.0e7));
    let near = lit.spawn(10.0, 10.0, LightOccluder2D::rect(4.0, 4.0));
    lit.frame();
    // It covers far more cells than an entry may hold: no cell lists it
    let index = lit.index();
    let Some(&slot) = index.slot_of.get(&huge) else {
      panic!("the huge occluder is indexed");
    };
    assert!(index.cells.values().all(|cell| !cell.contains(&slot)));
    assert_eq!(index.large, vec![slot]);
    let mut both = vec![huge, near];
    both.sort_by_key(entity_order);
    assert_eq!(lit.found_at(Vec2::new(10.0, 10.0)), both);
    assert_eq!(lit.found_at(Vec2::new(9.0e6, -9.0e6)), vec![huge]);
    assert!(lit.found_at(Vec2::new(1.1e7, 0.0)).is_empty());
    // Shrunk to a small one, it leaves the large ones for its cells
    lit
      .world
      .entity_mut(huge)
      .insert(LightOccluder2D::rect(4.0, 4.0));
    lit.frame();
    assert!(lit.index().large.is_empty());
    assert!(lit.found_at(Vec2::new(9.0e6, -9.0e6)).is_empty());
    assert_eq!(lit.found_at(Vec2::ZERO), vec![huge]);
  }

  #[test]
  fn queries_find_each_occluder_once_in_entity_order() {
    let mut lit = Lit::new();
    let mut spawned: Vec<Entity> = (0..20)
      .map(|i| {
        let x = (i % 5) as f32 * 700.0 - 1400.0;
        let y = (i / 5) as f32 * 700.0 - 1400.0;
        lit.spawn(x, y, LightOccluder2D::rect(600.0, 600.0))
      })
      .collect();
    // Some despawned and spawned again, so indices are reused out of order
    for &gone in &spawned[3..6] {
      lit.world.despawn(gone);
    }
    lit.frame();
    spawned.drain(3..6);
    spawned.push(lit.spawn(0.0, 0.0, LightOccluder2D::rect(3000.0, 3000.0)));
    lit.frame();
    spawned.sort_by_key(entity_order);
    for _ in 0..2 {
      assert_eq!(
        lit.found(Vec2::splat(-5000.0), Vec2::splat(5000.0)),
        spawned
      );
    }
    // Nested queries agree
    let index = lit.index();
    for outer in index.query(Vec2::splat(-100.0), Vec2::splat(100.0)) {
      let inner: Vec<Entity> = index
        .query(Vec2::splat(-5000.0), Vec2::splat(5000.0))
        .collect();
      assert_eq!(inner, spawned, "inside {outer}");
    }
  }

  #[test]
  fn a_static_scene_updates_nothing() {
    let mut lit = Lit::new();
    lit.spawn(0.0, 0.0, LightOccluder2D::rect(40.0, 40.0));
    let parent = lit.world.spawn(Transform::from_xy(600.0, 0.0)).id();
    lit.world.spawn((
      Transform::from_xy(10.0, 0.0),
      LightOccluder2D::rect(4.0, 4.0),
      crate::core::ecs::ChildOf(parent),
    ));
    let swinging = lit
      .world
      .spawn((
        Transform::from_xy(-600.0, 0.0),
        Interpolated,
        LightOccluder2D::rect(40.0, 4.0),
      ))
      .id();
    lit.world.entity_mut(swinging).insert(Disabled);
    lit.step();
    lit.world.entity_mut(swinging).remove::<Disabled>();
    lit.step();
    lit.move_to(swinging, -500.0, 0.0);
    lit.step();
    lit.step();
    lit.step();
    let settled = lit.index().updates();
    assert!(settled > 0);
    for _ in 0..10 {
      lit.step();
      lit.frame();
    }
    assert_eq!(lit.index().updates(), settled);
  }

  #[test]
  fn a_rotating_interpolated_occluder_is_found_along_its_swing() {
    let mut lit = Lit::new();
    // A thin bar 400 px long along x, turned to lie along y in one step
    let door = lit
      .world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        Interpolated,
        LightOccluder2D::rect(400.0, 4.0),
      ))
      .id();
    lit.step();
    lit.step();
    if let Some(mut transform) = lit.world.get_mut::<Transform>(door) {
      transform.rotation = FRAC_PI_2;
    }
    lit.step();
    // Halfway through the swing it lies along the diagonal
    let midway = Vec2::new(120.0, 120.0);
    assert_eq!(lit.found_at(midway), vec![door]);
    // Every blend of the swing is inside its bounds
    let Some((min, max)) = lit.index().bounds(door) else {
      panic!("the door is indexed");
    };
    for k in 0..=20 {
      let (Some(global), Some(previous)) = (
        lit.world.get::<GlobalTransform>(door),
        lit.world.get::<PreviousGlobalTransform>(door),
      ) else {
        panic!("the door is interpolated");
      };
      let drawn = render_global(global, Some(previous), k as f32 / 20.0);
      let Some((a, b)) = lit
        .world
        .get::<LightOccluder2D>(door)
        .and_then(|occluder| occluder.placed_bounds(&drawn))
      else {
        panic!("the door is placed");
      };
      assert!(
        a.cmpge(min - 1e-3).all() && b.cmple(max + 1e-3).all(),
        "at {k}/20: {a} {b} outside {min} {max}"
      );
    }
    // The step after, its previous placement catches up: it is found only
    // where it now stands
    lit.step();
    assert!(lit.found_at(midway).is_empty());
    assert_eq!(lit.found_at(Vec2::new(0.0, 190.0)), vec![door]);
  }

  /// A map of `size` 16 px solid cells' tiles, one layer called "ground"
  /// with a floor along its bottom row
  fn floor_map(world: &mut World, size: UVec2) -> crate::core::asset::Handle<TileMap> {
    let mut blocks = Tileset::new("blocks", UVec2::splat(16), TilesetImage::Collection);
    blocks.insert_tile(
      0,
      TileData {
        shapes: vec![TileShape {
          points: vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(16.0, 0.0),
            Vec2::new(16.0, 16.0),
            Vec2::new(0.0, 16.0),
          ],
          ..TileShape::default()
        }],
        ..TileData::default()
      },
    );
    let mut map = TileMap::new(size, UVec2::splat(16));
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(blocks)));
    let layer = map.add_tile_layer("ground");
    if let Some(grid) = map.tiles_mut(layer) {
      for x in 0..size.x as i32 {
        grid.set(IVec2::new(x, size.y as i32 - 1), Some(Tile::new(set, 0)));
      }
    }
    world.resource_mut::<Assets<TileMap>>().add(map)
  }

  fn map_occluders(world: &mut World) -> Vec<Entity> {
    let mut all: Vec<Entity> = world
      .query_filtered::<Entity, With<MapOccluder>>()
      .iter(world)
      .collect();
    all.sort_by_key(entity_order);
    all
  }

  #[test]
  fn map_chunk_chains_are_indexed_and_follow_an_edit() {
    let mut maps = MapWorld::new();
    insert_occluder_index(&mut maps.world);
    add_index_systems(maps.schedules());
    let handle = floor_map(&mut maps.world, UVec2::new(64, 32));
    let map = maps.spawn(MapRoot::new(handle));
    let whole = (Vec2::splat(-16.0), Vec2::new(1040.0, 528.0));
    let before = map_occluders(&mut maps.world);
    assert!(!before.is_empty());
    let index = maps.world.resource::<OccluderIndex>();
    assert_eq!(index.query(whole.0, whole.1).collect::<Vec<_>>(), before);
    let floor: Vec<Entity> = index
      .query(Vec2::new(100.0, 500.0), Vec2::new(102.0, 502.0))
      .collect();
    assert_eq!(floor.len(), 1, "the near chunk's floor");
    // A block in the air: its chunk is built again, and found in that frame
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(5, 10),
      Some(Tile::new(0, 0))
    ));
    maps.post_update();
    let after = map_occluders(&mut maps.world);
    let index = maps.world.resource::<OccluderIndex>();
    assert_eq!(index.query(whole.0, whole.1).collect::<Vec<_>>(), after);
    let block: Vec<Entity> = index
      .query(Vec2::new(86.0, 166.0), Vec2::new(90.0, 170.0))
      .collect();
    assert_eq!(block.len(), 1, "the block's chain");
    assert!(after.contains(&block[0]) && !before.contains(&block[0]));
    let Some((min, max)) = index.bounds(block[0]) else {
      panic!("the block's chain is indexed");
    };
    assert!(
      (min - Vec2::new(80.0, 160.0)).abs().max_element() < 1e-3,
      "{min}"
    );
    assert!(
      (max - Vec2::new(96.0, 176.0)).abs().max_element() < 1e-3,
      "{max}"
    );
    // The chunk's old floor chain is gone with it
    assert!(index.bounds(floor[0]).is_none());
  }
}
