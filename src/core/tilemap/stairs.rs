//! Stairs: how a character changes floor (spec "Floors: details settled at
//! planning", Stairs).
//!
//! A run is a set of stair cells (tiles whose `TileData::stairs` names a
//! higher floor) on one tile layer, joined through their sides, leading up
//! from the layer's floor to the floor its tiles name. Its top is the
//! middle of the sides it shares with that floor's footprint, and its
//! bottom its foot: the side it shares with neither that is farthest from
//! its top along the run. A map with floors keeps its runs in a
//! [`MapStairs`] beside its [`MapFloors`], brought up to its tiles and
//! tilesets each fixed step ([`sync_stair_runs`]), with an index of their
//! cells, so a climber's run is looked up where it stands.
//!
//! An entity with [`Climber`] changes floor halfway along a run
//! ([`climb_stairs`]), and draws eased between the two floors' heights by
//! how far up it is (`floors::lift_floored`).

use std::sync::Arc;

use glam::{IVec2, Vec2};

use super::collision::{FootprintRule, NoFootprint};
use super::floors::{self, MapArea, MapFloors, ground_offset};
use super::kinds::{
  Footing, GridKey, Layers, Placement, StairSource, Stores, TilesetStamp, Tracked, changed_chunks,
  chunk_origin, stair_sources, stairs_of, stamps, stamps_match,
};
use super::spawn::{MapLayers, MapRoot, root_map};
use super::{CHUNK_SIZE, LayerInfo, MapGrid, TileGrid, TileLayerData, TileMap, Tileset};
use crate::core::asset::Assets;
use crate::core::ecs::{
  Allow, Commands, Component, DetectChanges, Disabled, Entity, Floor, GlobalTransform, Local,
  Message, MessageWriter, Query, Ref, With, Without,
};
use crate::core::hash::{FastMap, FastSet};

/// An entity whose floor changes on stairs, and whose drawn height eases
/// along them.
///
/// On a run of stairs (tiles whose `stairs` property names the floor they
/// lead up to: [`TileData::stairs`](super::TileData::stairs)), a climber on
/// either of the run's floors goes onto the upper floor once it is halfway
/// up, and back onto the lower once it is halfway down, each change sent as
/// a [`FloorChanged`]. All along the run it draws lifted between the two
/// floors' heights by how far up it is. Its `Transform` stays where it
/// stands on the ground, as on any floor; an entity without `Climber` keeps
/// its floor on stairs.
///
/// ```
/// use ryo2d::prelude::*;
///
/// /// A top-down hero who walks up and down the map's stairs, its sprite a
/// /// child that draws at its height
/// fn spawn_hero(ctx: &mut SceneContext<'_>, at: Vec2, art: Handle<Image>) -> Entity {
///   let hero = ctx
///     .world
///     .spawn((
///       Transform::from_translation(at),
///       Collider::circle(5.0),
///       TopDownMovement::default(),
///       Climber,
///     ))
///     .id();
///   ctx.world.spawn((
///     Transform::from_xy(0.0, -8.0),
///     Renderable::image(art),
///     ChildOf(hero),
///   ));
///   hero
/// }
/// ```
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[require(Floor)]
pub struct Climber;

/// A [`Climber`] changed floor on stairs. Written in the fixed step before
/// physics, so that step's physics already has it on `to`; `update` reads
/// it the step after.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloorChanged {
  /// The climber
  pub entity: Entity,
  /// The floor it was on
  pub from: Floor,
  /// The floor it is on now
  pub to: Floor,
}

/// A run of stairs on a map: stair cells of one tile layer leading to one
/// floor, joined through their sides
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StairRun {
  /// The floor it leads up from (its layer's), and the floor it leads to
  pub(crate) from: u8,
  pub(crate) to: u8,
  /// Its cells on its layer's grid, by row
  pub(crate) cells: Vec<IVec2>,
  /// Its foot and its top, in the map's pixels in ground space
  pub(crate) bottom: Vec2,
  pub(crate) top: Vec2,
  /// Its layer's grid, and where the layer stands in the map
  pub(crate) grid: MapGrid,
  pub(crate) offset: Vec2,
}

impl StairRun {
  /// Whether the map point `at` (ground space) is on one of its cells. The
  /// tests' look at every run in turn, which `MapStairs::climbing`'s index
  /// answers as
  #[cfg(test)]
  pub(crate) fn holds(&self, at: Vec2) -> bool {
    let cell = self.grid.cell_at(at - self.offset);
    self
      .cells
      .binary_search_by_key(&(cell.y, cell.x), |c| (c.y, c.x))
      .is_ok()
  }

  /// How far up it the map point `at` is: 0 at its bottom, 1 at its top
  pub(crate) fn progress(&self, at: Vec2) -> f32 {
    let up = self.top - self.bottom;
    ((at - self.bottom).dot(up) / up.length_squared()).clamp(0.0, 1.0)
  }
}

impl MapStairs {
  /// The run under the map point `at` (ground space) that something on
  /// `floor` climbs, one of its two floors, and how far up it `at` is: the
  /// first such run in `runs`, looked up by the cell `at` is in at each
  /// place runs stand, so it costs the same however many runs the map has,
  /// and allocates nothing
  pub(crate) fn climbing(&self, at: Vec2, floor: Floor) -> Option<(&StairRun, f32)> {
    let number = floor.number()?;
    let mut first: Option<usize> = None;
    for place in &self.index {
      let cell = place.grid.cell_at(at - place.offset);
      let start = place
        .cells
        .partition_point(|&(of, _)| (of.y, of.x) < (cell.y, cell.x));
      let found = place.cells[start..]
        .iter()
        .take_while(|&&(of, _)| of == cell)
        .map(|&(_, run)| run)
        .find(|&run| {
          self
            .runs
            .get(run)
            .is_some_and(|run| run.from == number || run.to == number)
        });
      first = match (first, found) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
      };
    }
    let run = self.runs.get(first?)?;
    Some((run, run.progress(at)))
  }

  /// Its runs made again: `runs`, and the index of their cells
  fn put(&mut self, runs: Vec<StairRun>) {
    self.index = index_of(&runs);
    self.runs = runs;
  }
}

/// Where some of a map's runs stand (their layers' grid and ground
/// offset), and their cells there
#[derive(Debug)]
struct StairPlace {
  key: (GridKey, [u32; 2]),
  grid: MapGrid,
  offset: Vec2,
  /// Each cell of a run standing here, with the run's index in the runs,
  /// by row, then by run: a cell in several runs (up from one floor and on
  /// from the next, on two layers) is here once for each, in their order
  cells: Vec<(IVec2, usize)>,
}

/// The index of `runs`' cells: the places they stand at, each with its
/// runs' cells
fn index_of(runs: &[StairRun]) -> Vec<StairPlace> {
  let mut places: Vec<StairPlace> = Vec::new();
  for (index, run) in runs.iter().enumerate() {
    let key = (
      GridKey::of(&run.grid),
      [run.offset.x.to_bits(), run.offset.y.to_bits()],
    );
    let at = match places.iter().position(|place| place.key == key) {
      Some(at) => at,
      None => {
        places.push(StairPlace {
          key,
          grid: run.grid,
          offset: run.offset,
          cells: Vec::new(),
        });
        places.len() - 1
      }
    };
    places[at]
      .cells
      .extend(run.cells.iter().map(|&cell| (cell, index)));
  }
  for place in &mut places {
    place
      .cells
      .sort_unstable_by_key(|&(cell, run)| (cell.y, cell.x, run));
  }
  places
}

/// A tile layer the runs read: its entity, floor and where it stands (its
/// grid, and its ground offset by its bits)
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Place {
  entity: Entity,
  floor: u8,
  grid: GridKey,
  offset: [u32; 2],
}

impl Place {
  fn of(entity: Entity, info: &LayerInfo, grid: &MapGrid, floors: &MapFloors) -> Place {
    let offset = ground_offset(info, Some(floors));
    Place {
      entity,
      floor: info.floor,
      grid: GridKey::of(grid),
      offset: [offset.x.to_bits(), offset.y.to_bits()],
    }
  }

  /// What it gives floor `to`'s footprint, by the floor's rule
  fn footing(&self, rule: &FootprintRule) -> Option<Footing> {
    rule.footing(self.floor, &self.grid.grid(), self.offset())
  }

  fn offset(&self) -> Vec2 {
    Vec2::new(
      f32::from_bits(self.offset[0]),
      f32::from_bits(self.offset[1]),
    )
  }
}

/// A tile layer as the runs read it, and the stair cells it held
struct ReadLayer {
  place: Place,
  tracked: Tracked,
  /// The stair cells of each chunk holding any, each with the floor it
  /// leads to
  cells: FastMap<IVec2, Vec<(IVec2, u8)>>,
}

impl ReadLayer {
  /// Reads again the chunks of its layer whose version moved since it last
  /// looked (every chunk, the first time), by the tilesets' `stairs`;
  /// whether any did
  fn read_changes(&mut self, layers: &Layers, stairs: &[Option<StairSource>]) -> bool {
    let changed = changed_chunks(std::slice::from_mut(&mut self.tracked), layers, &[]);
    if changed.is_empty() {
      return false;
    }
    let Ok((_, data, _)) = layers.get(self.place.entity) else {
      return false;
    };
    // A grid that places nothing gives a footprint no cells, nor stairs
    let placed = placed(&data.grid);
    for chunk in changed {
      self.cells.remove(&chunk);
      if !placed {
        continue;
      }
      let found = stair_cells(&data.tiles, chunk, stairs);
      if !found.is_empty() {
        self.cells.insert(chunk, found);
      }
    }
    true
  }
}

/// Whether `grid` places its cells (`Placement`): a lattice that folds them
/// flat, or hexagons the collision tracer does not take, place none
fn placed(grid: &MapGrid) -> bool {
  !matches!(Placement::of(grid), Placement::Nothing)
}

/// The cells of `chunk` of `tiles` holding stairs (by the tilesets'
/// `stairs`), each with the floor it leads to, by row
fn stair_cells(tiles: &TileGrid, chunk: IVec2, stairs: &[Option<StairSource>]) -> Vec<(IVec2, u8)> {
  let (Some(origin), Some(cells)) = (chunk_origin(chunk), tiles.chunk_cells(chunk)) else {
    return Vec::new();
  };
  let side = CHUNK_SIZE as usize;
  let mut found = Vec::new();
  // The cell value met last and the floor it leads to: a chunk holds few
  // tiles, often over and over
  let mut met = (0u32, None);
  for (index, &raw) in cells.iter().enumerate() {
    if raw == 0 {
      continue;
    }
    if raw != met.0 {
      let to = TileGrid::decode(raw).and_then(|tile| stairs_of(stairs, tile));
      met = (raw, to);
    }
    if let Some(to) = met.1 {
      let within = IVec2::new((index % side) as i32, (index / side) as i32);
      found.push((origin + within, to));
    }
  }
  found
}

/// A map's runs of stairs, on the map entity beside its `MapFloors`, with
/// the index of their cells, and what they were read from: each tile layer
/// with the stair cells its chunks held, its tilesets' stairs, and what it
/// has warned of
#[derive(Component, Default)]
pub(crate) struct MapStairs {
  /// The runs, as its tiles were when last read (written with `put`, which
  /// indexes them)
  pub(crate) runs: Vec<StairRun>,
  /// Their cells, by place
  index: Vec<StairPlace>,
  layers: Vec<ReadLayer>,
  tilesets: Vec<TilesetStamp>,
  stairs: Arc<[Option<StairSource>]>,
  warned: FastSet<&'static str>,
}

impl MapStairs {
  /// Whether the stairs read still stand for `map`'s tilesets: their stamps
  /// match, or they moved and give the same stairs. Brought up to date
  /// either way
  fn stairs_kept(&mut self, map: &TileMap, assets: Option<&Assets<Tileset>>) -> bool {
    if stamps_match(&self.tilesets, map, assets) {
      return true;
    }
    let now = stair_sources(map, assets);
    let same = *now == *self.stairs;
    self.stairs = now;
    self.tilesets = stamps(map, assets);
    same
  }
}

/// The maps whose floors went, whose `MapStairs` goes with them
type Unfloored<'w, 's> =
  Query<'w, 's, Entity, (With<MapStairs>, Without<MapFloors>, Allow<Disabled>)>;

/// The maps with floors, their runs brought up to their tiles
type StairMaps<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static MapRoot,
    &'static MapLayers,
    Ref<'static, MapFloors>,
    Option<&'static mut MapStairs>,
  ),
>;

/// Each map's stairs, by its entity
pub(super) type Stairs<'w, 's> = Query<'w, 's, &'static MapStairs>;

/// `FixedSet::Movement`, before `climb_stairs`: each map with floors' runs
/// brought up to its tiles and tilesets. The chunks whose version moved
/// since it last looked are read again, and every run made again from the
/// stair cells, and indexed, when any did; a change of what they are read
/// from (a new `MapFloors`, the map's tile layers, their floors, grids or
/// places, the tilesets' stairs) reads every chunk again. A disabled map is
/// left as it is, and caught up once enabled. A map without floors costs
/// nothing; an idle step, a look at each tile layer's version and the
/// tilesets' stamps
pub(crate) fn sync_stair_runs(
  mut maps: StairMaps,
  gone: Unfloored,
  layers: Layers,
  (tile_maps, projects, tilesets): Stores,
  mut places: Local<Vec<(usize, Place)>>,
  mut commands: Commands,
) {
  for map in &gone {
    commands.entity(map).try_remove::<MapStairs>();
  }
  let tilesets = tilesets.as_deref();
  for (map, root, map_layers, floors, state) in &mut maps {
    let Some(tile_map) = root_map(root, tile_maps.as_deref(), projects.as_deref()) else {
      continue;
    };
    // Landed, or reloaded with other floors: its runs are to make
    let replaced = floors.is_changed();
    places.clear();
    places.extend(
      map_layers
        .0
        .iter()
        .enumerate()
        .filter_map(|(index, &entity)| {
          let (info, data, disabled) = layers.get(entity).ok()?;
          (!disabled).then(|| (index, Place::of(entity, info, &data.grid, &floors)))
        }),
    );
    let mut fresh = None;
    let state = match state {
      Some(state) => state.into_inner(),
      None => fresh.insert(MapStairs::default()),
    };
    let kept = state.stairs_kept(tile_map, tilesets)
      && state.layers.len() == places.len()
      && state
        .layers
        .iter()
        .zip(places.iter())
        .all(|(read, (_, place))| read.place == *place);
    if replaced || !kept {
      state.layers = places
        .iter()
        .filter_map(|&(index, place)| {
          let (_, data, _) = layers.get(place.entity).ok()?;
          Some(ReadLayer {
            place,
            tracked: Tracked::unread((index, place.entity), &data, &[]),
            cells: FastMap::default(),
          })
        })
        .collect();
    }
    let MapStairs {
      layers: read,
      stairs,
      warned,
      ..
    } = &mut *state;
    let mut changed = false;
    for layer in read.iter_mut() {
      changed |= layer.read_changes(&layers, stairs);
    }
    // Every run is made again, whichever layer changed: a run's top and
    // foot depend on its upper floor's layers and the other layers' stairs
    // too
    if changed || replaced || !kept {
      let runs = runs_of(map, read, &layers, &floors, warned);
      state.put(runs);
    }
    if let Some(state) = fresh {
      commands.entity(map).try_insert(state);
    }
  }
}

/// The runs of a map's tile layers (`read`, its enabled tile layers in
/// order, with their stair cells) on its `floors`: the stair cells of each
/// layer leading to one floor, joined through their sides. Which layers
/// give a floor's footprint and its stairs is the floor's
/// `FootprintRule`, as for its edge. Stairs naming a floor the map lacks,
/// their own floor or a lower one stay plain tiles; a run leads nowhere,
/// and is left out, when the floor has no footprint, when its layer gives
/// the footprint no stairs (standing elsewhere), or with no top, no
/// bottom or no way up: each warned once a map (`warned`)
fn runs_of(
  map: Entity,
  read: &[ReadLayer],
  layers: &Layers,
  floors: &MapFloors,
  warned: &mut FastSet<&'static str>,
) -> Vec<StairRun> {
  let name = |layer: &ReadLayer| {
    layers
      .get(layer.place.entity)
      .map_or(String::new(), |(info, ..)| info.name.clone())
  };
  // Each floor's footprint rule, worked out once
  let mut rules: Vec<(u8, Result<FootprintRule, NoFootprint>)> = Vec::new();
  let mut rule_of = |to: u8| {
    if let Some(&(_, rule)) = rules.iter().find(|(of, _)| *of == to) {
      return rule;
    }
    let places = read.iter().map(|layer| {
      let place = &layer.place;
      (place.floor, place.grid.grid(), place.offset())
    });
    let rule = FootprintRule::of(to, places);
    rules.push((to, rule));
    rule
  };
  // The stair cells of each layer by the floor they lead to
  let mut leading: Vec<(usize, u8, Vec<IVec2>)> = Vec::new();
  for (index, layer) in read.iter().enumerate() {
    let from = layer.place.floor;
    for &(cell, to) in layer.cells.values().flatten() {
      if to <= from || floors.lift(Floor::On(to)).is_none() {
        if warned.insert("plain") {
          crate::engine_warn!(
            module: "Tilemap",
            "map {map}: stairs on layer \"{}\" lead to floor {to}, which is not a floor of the map above the layer's floor {from}, so they are plain tiles",
            name(layer)
          );
        }
        continue;
      }
      let why = match rule_of(to) {
        Ok(rule) if layer.place.footing(&rule) == Some(Footing::Stairs) => None,
        Ok(_) => Some(
          "they stand elsewhere than that floor's bottom tile layer, on another grid or at another offset",
        ),
        Err(NoFootprint::NoTileLayer) => Some("that floor has no tile layer to climb onto"),
        Err(NoFootprint::Untraceable(_)) => {
          Some("that floor's hexagons don't collide, so it has no edge to climb into")
        }
      };
      if let Some(why) = why {
        warn_nowhere(map, warned, (|| name(layer), to), why);
        continue;
      }
      match leading
        .iter_mut()
        .find(|(of, up, _)| *of == index && *up == to)
      {
        Some((.., cells)) => cells.push(cell),
        None => leading.push((index, to, vec![cell])),
      }
    }
  }
  // The stairs up to each floor, which its footprint holds
  let stairs_up: FastSet<(u8, IVec2)> = leading
    .iter()
    .flat_map(|(_, to, cells)| cells.iter().map(move |&cell| (*to, cell)))
    .collect();
  let mut runs = Vec::new();
  for (index, to, mut cells) in leading {
    let Ok(rule) = rule_of(to) else {
      continue;
    };
    let layer = &read[index];
    cells.sort_unstable_by_key(|c| (c.y, c.x));
    // The cells of floor `to`'s tile layers the rule takes whole: its
    // footprint, with its stairs
    let upper: Vec<&TileLayerData> = read
      .iter()
      .filter(|other| other.place.footing(&rule) == Some(Footing::Tiles))
      .filter_map(|other| layers.get(other.place.entity).ok())
      .map(|(_, data, _)| data.into_inner())
      .filter(|data| placed(&data.grid))
      .collect();
    let footprint = |cell: IVec2| {
      stairs_up.contains(&(to, cell)) || upper.iter().any(|data| data.tiles.get(cell).is_some())
    };
    let grid = layer.place.grid.grid();
    let offset = layer.place.offset();
    for run in joined(&grid, &cells) {
      match ends(&grid, &run, footprint) {
        Ok((bottom, top)) => runs.push(StairRun {
          from: layer.place.floor,
          to,
          cells: run,
          bottom: bottom + offset,
          top: top + offset,
          grid,
          offset,
        }),
        Err(why) => warn_nowhere(map, warned, (|| name(layer), to), why),
      }
    }
  }
  runs
}

/// Warns, once a map (`warned`), that stairs on a layer (`layer`, its
/// name) up to floor `to` lead nowhere, and `why`
fn warn_nowhere(
  map: Entity,
  warned: &mut FastSet<&'static str>,
  (layer, to): (impl FnOnce() -> String, u8),
  why: &str,
) {
  if warned.insert("nowhere") {
    crate::engine_warn!(
      module: "Tilemap",
      "map {map}: stairs on layer \"{}\" up to floor {to} lead nowhere ({why}), so they change no climber's floor",
      layer()
    );
  }
}

/// `cells` (by row) in the sets of them joined through their sides on
/// `grid`, each by row
fn joined(grid: &MapGrid, cells: &[IVec2]) -> Vec<Vec<IVec2>> {
  let all: FastSet<IVec2> = cells.iter().copied().collect();
  let mut seen: FastSet<IVec2> = FastSet::default();
  let mut sets = Vec::new();
  for &start in cells {
    if !seen.insert(start) {
      continue;
    }
    let mut set = vec![start];
    let mut next = 0;
    while let Some(&cell) = set.get(next) {
      next += 1;
      set.extend(
        grid
          .neighbours(cell)
          .filter(|&near| all.contains(&near) && seen.insert(near)),
      );
    }
    set.sort_unstable_by_key(|c| (c.y, c.x));
    sets.push(set);
  }
  sets
}

/// The bottom and top of the run `run` (by row) on `grid`, in its layer's
/// pixels. Its top is the middle of the sides it shares with its upper
/// floor's footprint (`footprint`). It climbs from the middle of the sides
/// it shares with cells outside both toward its top, and its bottom is its
/// foot: the farthest of those sides along that line, so its lift eases
/// over its whole length (the middle alone takes in the sides along it).
/// `Err`, saying why, for a run with no top, no bottom, or no way up
fn ends(
  grid: &MapGrid,
  run: &[IVec2],
  footprint: impl Fn(IVec2) -> bool,
) -> Result<(Vec2, Vec2), &'static str> {
  let (mut top, mut tops) = (Vec2::ZERO, 0u32);
  let mut outside = Vec::new();
  for &cell in run {
    let centre = grid.cell_center(cell);
    for near in grid.neighbours(cell) {
      if run
        .binary_search_by_key(&(near.y, near.x), |c| (c.y, c.x))
        .is_ok()
      {
        continue;
      }
      // Two cells' shared side's middle is halfway between their centres
      let side = (centre + grid.cell_center(near)) / 2.0;
      if footprint(near) {
        top += side;
        tops += 1;
      } else {
        outside.push(side);
      }
    }
  }
  if tops == 0 {
    return Err("they share no side with that floor's cells");
  }
  if outside.is_empty() {
    return Err("they share every side with that floor's cells");
  }
  let top = top / tops as f32;
  let middle = outside.iter().sum::<Vec2>() / outside.len() as f32;
  if (top - middle).length() < 1e-3 {
    return Err("they meet that floor as much on one side as on the other");
  }
  let up = (top - middle).normalize();
  let foot = outside
    .iter()
    .map(|&side| (side - top).dot(up))
    .fold(0.0, f32::min);
  Ok((top + up * foot, top))
}

/// `FixedSet::Movement`, after `sync_stair_runs` and before physics: each
/// climber on a run, on one of its two floors, goes onto its upper floor
/// halfway up it or more, and onto its lower floor below that, by where the
/// last step left it (`GlobalTransform`) on the map with floors under it.
/// A change is written to its `Floor` (which this step's physics filters
/// by) and sent as a [`FloorChanged`]. A climber whose `Floor` something
/// else wrote since this last ran (a game's teleport in `update`) is left
/// to that write for the step: the stairs still see it where it stood.
/// Ordered before the kill zones' respawns, which then win over it in the
/// step. With no climber, or no map with stairs, it returns at once
pub(crate) fn climb_stairs(
  mut climbers: Query<(Entity, &mut Floor, &GlobalTransform), With<Climber>>,
  (maps, tiles, stairs): (floors::Maps, floors::TileLayers, Stairs),
  mut changes: MessageWriter<FloorChanged>,
  mut areas: Local<Vec<MapArea>>,
) {
  if climbers.is_empty() || stairs.iter().all(|stairs| stairs.runs.is_empty()) {
    return;
  }
  floors::find_areas(&mut areas, &maps, &tiles);
  for (entity, mut floor, global) in &mut climbers {
    // Set by something else since the stairs last looked: that write wins
    if floor.is_changed() {
      continue;
    }
    let at = global.translation();
    let Some(area) = areas.iter().find(|area| area.holds(at)) else {
      continue;
    };
    let Ok(map_stairs) = stairs.get(area.map) else {
      continue;
    };
    let Some((run, up)) = map_stairs.climbing(area.local(at), *floor) else {
      continue;
    };
    let now = Floor::On(if up >= 0.5 { run.to } else { run.from });
    if *floor != now {
      changes.write(FloorChanged {
        entity,
        from: *floor,
        to: now,
      });
      *floor = now;
    }
  }
}

#[cfg(test)]
mod tests;
